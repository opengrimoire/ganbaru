use super::fixtures::*;
use crate::events::commit::{CommitRequest, prepare_rows};
use crate::events::edit::{EventDraft, prepare};
use crate::events::scope::read_snapshot;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use serde_json::{Value, json};

async fn accepted_retry_fixture() -> (
    sqlx::SqlitePool,
    CommitRequest,
    crate::events::commit::CommitReceipt,
) {
    let pool = super::metadata::seed().await;
    let now = clock("2099-05-08T00:00:00Z");
    let draft = json!({"fields":[{"field":"title","value":"Accepted"}]});
    let (request, revision) = review(&pool, draft.clone(), EditScope::This, now).await;
    let mut tx = pool.begin().await.unwrap();
    let rows = prepare_rows(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        EditScope::This,
        now,
        serde_json::from_value(draft).unwrap(),
        "save-1",
        &revision,
    )
    .unwrap();
    let receipt = rows.write(&mut tx).await.unwrap();
    request
        .record_receipt(&mut tx, &receipt, now.epoch_ms)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (pool, request, receipt)
}

#[test]
fn read_only_retry_returns_accepted_result_after_source_removal_and_generation_change() {
    crate::test_support::block_on(async {
        let (pool, request, receipt) = accepted_retry_fixture().await;
        sqlx::query("DELETE FROM calendar_events WHERE id = 'source'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA query_only = ON")
            .execute(&pool)
            .await
            .unwrap();
        let before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mut value = serde_json::to_value(request).unwrap();
        value["vaultGeneration"] = json!(2);
        let retry: CommitRequest = serde_json::from_value(value).unwrap();
        for _ in 0..2 {
            let actual = retry.read_only_retry(&pool).await.unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(&receipt).unwrap()
            );
        }
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, after);
        assert!(
            sqlx::query("DELETE FROM calendar_edit_receipts")
                .execute(&pool)
                .await
                .is_err()
        );
    });
}

#[test]
fn read_only_retry_cannot_execute_absent_or_changed_intent() {
    crate::test_support::block_on(async {
        let (pool, request, _) = accepted_retry_fixture().await;
        sqlx::query("PRAGMA query_only = ON")
            .execute(&pool)
            .await
            .unwrap();
        let before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mut value = serde_json::to_value(&request).unwrap();
        value["commandId"] = json!("not-accepted");
        let absent: CommitRequest = serde_json::from_value(value).unwrap();
        assert!(absent.read_only_retry(&pool).await.unwrap().is_none());
        let mut value = serde_json::to_value(request).unwrap();
        value["edit"]["draft"]["fields"][0]["value"] = json!("Different");
        let changed: CommitRequest = serde_json::from_value(value).unwrap();
        let error = changed.read_only_retry(&pool).await.err().unwrap();
        let error = serde_json::to_value(error).unwrap();
        assert_eq!(error["outcome"], "unknown");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("different intent")
        );
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, after);
        let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_edit_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 1);
    });
}

#[test]
fn read_only_retry_retains_unknown_outcome_for_unreadable_or_oversized_receipts() {
    crate::test_support::block_on(async {
        let (pool, request, _) = accepted_retry_fixture().await;
        sqlx::query(
            "UPDATE calendar_edit_receipts SET result_json = ? WHERE command_id = 'save-1'",
        )
        .bind(" ".repeat(crate::events::commit::MAX_RECEIPT_BYTES + 1))
        .execute(&pool)
        .await
        .unwrap();
        let error =
            serde_json::to_value(request.read_only_retry(&pool).await.err().unwrap()).unwrap();
        assert_eq!(error["outcome"], "unknown");
        assert!(error["message"].as_str().unwrap().contains("byte limit"));
        sqlx::query("UPDATE calendar_edit_receipts SET result_json = 'invalid json' WHERE command_id = 'save-1'")
            .execute(&pool).await.unwrap();
        let error =
            serde_json::to_value(request.read_only_retry(&pool).await.err().unwrap()).unwrap();
        assert_eq!(error["outcome"], "unknown");
        assert!(error["message"].as_str().unwrap().contains("decode"));
        pool.close().await;
        let error =
            serde_json::to_value(request.read_only_retry(&pool).await.err().unwrap()).unwrap();
        assert_eq!(error["outcome"], "unknown");
    });
}

#[test]
fn end_now_review_retains_semantic_intent_across_native_clock_changes() {
    use crate::events::edit::{EditAction, prepare_action};
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        let mut revisions = Vec::new();
        for instant in ["2099-05-10T10:05:00Z", "2099-05-10T10:06:00.123Z"] {
            let now = clock(instant);
            let prepared = prepare_action(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::This,
                now,
                EventDraft::default(),
                EditAction::EndNow,
            )
            .unwrap();
            assert_eq!(prepared.selected_after.end_ms, now.epoch_ms);
            revisions.push(prepared.review_revision);
        }
        assert_eq!(revisions[0], revisions[1]);
        for (instant, scope, draft) in [
            ("2099-05-10T09:59:00Z", EditScope::This, json!({})),
            ("2099-05-10T11:00:00Z", EditScope::This, json!({})),
            ("2099-05-10T10:05:00Z", EditScope::All, json!({})),
            (
                "2099-05-10T10:05:00Z",
                EditScope::This,
                json!({"timing":{"endTime":"2099-05-10T10:06:00Z"}}),
            ),
            (
                "2099-05-10T10:05:00Z",
                EditScope::This,
                json!({"timing":{"startTime":"2099-05-10T10:01:00Z"}}),
            ),
            (
                "2099-05-10T10:05:00Z",
                EditScope::This,
                json!({"recurrence":{"kind":"clear"}}),
            ),
        ] {
            assert!(
                prepare_action(
                    read_snapshot(&mut tx, "source").await.unwrap(),
                    parse_date("2099-05-10").unwrap(),
                    scope,
                    clock(instant),
                    serde_json::from_value(draft).unwrap(),
                    EditAction::EndNow,
                )
                .is_err(),
                "{instant}"
            );
        }
        sqlx::query("UPDATE calendar_events SET title = 'Concurrent title' WHERE id = 'source'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let prepared = prepare_action(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            clock("2099-05-10T10:07:00Z"),
            EventDraft::default(),
            EditAction::EndNow,
        )
        .unwrap();
        assert_ne!(prepared.review_revision, revisions[0]);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn end_now_detach_completion_and_receipt_commit_or_roll_back_together() {
    use crate::events::commit::check_review;
    use crate::events::edit::{EditAction, prepare_action};
    use ganbaru_pomodoro::*;
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let started = clock("2099-05-10T10:05:00Z");
        let accepted = clock("2099-05-10T10:06:00.123Z");
        let mut tx = pool.begin().await.unwrap();
        let calendar = crate::reads::focus_context::resolve(
            &mut tx,
            started.epoch_ms,
            Some("source::2099-05-10"),
        )
        .await
        .unwrap();
        let mut context = FocusExecutionContext {
            now_ms: started.epoch_ms,
            platform: FocusPlatform::Desktop,
            foreground: true,
            commitment: calendar.commitment,
            planned_blocks: Vec::new(),
            local_time: None,
        };
        let before = focus_execute_command_tx(
            &mut tx,
            &FocusCommand {
                command_id: "start-end-now".into(),
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
        let mut tx = pool.begin().await.unwrap();
        let preview = prepare_action(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            started,
            EventDraft::default(),
            EditAction::EndNow,
        )
        .unwrap();
        let revision = preview.review_revision;
        tx.rollback().await.unwrap();
        let request: CommitRequest = serde_json::from_value(json!({
            "vaultId":"vault", "vaultGeneration":1, "commandId":"end-now", "reviewRevision":revision,
            "edit":{"selection":{"templateId":"source","recurrenceDate":"2099-05-10","scope":"this"},
                "draft":{},"action":"end_now"}
        })).unwrap();
        context.now_ms = accepted.epoch_ms;
        context.commitment = None;
        for inject_failure in [true, false] {
            if inject_failure {
                sqlx::query("CREATE TRIGGER reject_end_receipt BEFORE INSERT ON calendar_edit_receipts BEGIN SELECT RAISE(ABORT, 'receipt failed'); END").execute(&pool).await.unwrap();
            }
            let mut tx = pool.begin().await.unwrap();
            assert!(request.read_receipt(&mut tx).await.unwrap().is_none());
            let prepared = prepare_action(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::This,
                accepted,
                EventDraft::default(),
                EditAction::EndNow,
            )
            .unwrap();
            let prepared = check_review(prepared, accepted, "end-now", &revision).unwrap();
            let (start_ms, end_ms) = prepared.completion_window.unwrap();
            let reference = prepared.active_change(&before).unwrap().unwrap();
            let receipt = prepared.write(&mut tx).await.unwrap();
            let result = focus_complete_calendar_tx(
                &mut tx,
                before.revision,
                &FocusCalendarCompletion {
                    reference,
                    start_ms,
                    end_ms,
                },
                &context,
            )
            .await
            .unwrap();
            let recorded = request
                .record_receipt(&mut tx, &receipt, accepted.epoch_ms)
                .await;
            if inject_failure {
                assert!(recorded.unwrap_err().contains("receipt failed"));
                tx.rollback().await.unwrap();
                let unchanged = focus_read_execution_snapshot(&pool, accepted.epoch_ms)
                    .await
                    .unwrap();
                assert_eq!(unchanged.run, before.run);
                assert_eq!(unchanged.segment, before.segment);
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(count, 2);
                sqlx::query("DROP TRIGGER reject_end_receipt")
                    .execute(&pool)
                    .await
                    .unwrap();
            } else {
                recorded.unwrap();
                tx.commit().await.unwrap();
                let run = result.run.unwrap();
                assert_eq!(run.ended_at_ms, Some(accepted.epoch_ms));
                assert_eq!(run.planned_end_ms, accepted.epoch_ms);
                assert_eq!(run.occurrence_id, receipt.edited_id);
                let end: String =
                    sqlx::query_scalar("SELECT end_time FROM calendar_events WHERE id = ?")
                        .bind(&receipt.edited_id)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert_eq!(clock(&end).epoch_ms, accepted.epoch_ms);
                let mut tx = pool.begin().await.unwrap();
                let retry = request.read_receipt(&mut tx).await.unwrap().unwrap();
                assert_eq!(retry.edited_id, receipt.edited_id);
                tx.rollback().await.unwrap();
            }
        }
    });
}

fn clock(value: &str) -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis(),
        floating_today: None,
    }
}

#[test]
fn enable_focus_configuration_start_and_receipts_are_atomic_after_an_earlier_run() {
    use crate::events::commit::check_review;
    use crate::events::edit::{EditAction, prepare_action};
    use ganbaru_pomodoro::*;
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::raw_sql("UPDATE calendar_events SET start_time = '2099-05-10T10:00:00Z', end_time = '2099-05-10T11:00:00Z', rrule = NULL WHERE id = 'source'; DELETE FROM calendar_event_pomodoro_configs WHERE event_id = 'source';").execute(&pool).await.unwrap();
        insert_test_completed_pomodoro_history(&pool, "source", "source", "2099-05-10").await;
        let now = clock("2099-05-10T10:45:00Z");
        let draft = json!({"pomodoroConfig":{"action":"set","value":sequence_pomodoro_config()}});
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_action(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            now,
            serde_json::from_value(draft.clone()).unwrap(),
            EditAction::EnableFocus,
        )
        .unwrap();
        let review = prepared.review_revision;
        tx.rollback().await.unwrap();
        let request: CommitRequest = serde_json::from_value(json!({
            "vaultId":"vault", "vaultGeneration":1, "commandId":"enable", "reviewRevision":review,
            "edit":{"selection":{"templateId":"source","recurrenceDate":"2099-05-10","scope":"this"},
                "draft":draft,"action":"enable_focus"}
        })).unwrap();
        for inject_failure in [true, false] {
            if inject_failure {
                sqlx::query("CREATE TRIGGER reject_enabled_phase BEFORE INSERT ON pomodoro_segments BEGIN SELECT RAISE(ABORT, 'phase failed'); END").execute(&pool).await.unwrap();
            }
            let mut tx = pool.begin().await.unwrap();
            let before = focus_read_execution_snapshot_tx(&mut tx, now.epoch_ms)
                .await
                .unwrap();
            let prepared = prepare_action(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::This,
                now,
                serde_json::from_value(draft.clone()).unwrap(),
                EditAction::EnableFocus,
            )
            .unwrap();
            let prepared = check_review(prepared, now, "enable", &review).unwrap();
            let occurrence = prepared.start_occurrence.clone().unwrap();
            let command = request
                .start_focus_command(occurrence.clone(), &before)
                .unwrap();
            let receipt = prepared.write(&mut tx).await.unwrap();
            let calendar =
                crate::reads::focus_context::resolve(&mut tx, now.epoch_ms, Some(&occurrence))
                    .await
                    .unwrap();
            let context = FocusExecutionContext {
                now_ms: now.epoch_ms,
                platform: FocusPlatform::Desktop,
                foreground: true,
                commitment: calendar.commitment,
                planned_blocks: Vec::new(),
                local_time: None,
            };
            let result = focus_execute_command_tx(&mut tx, &command, &context).await;
            if inject_failure {
                assert!(result.unwrap_err().message.contains("phase failed"));
                tx.rollback().await.unwrap();
                let configs: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM calendar_event_pomodoro_configs WHERE event_id = 'source'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(configs, 0);
                let mut tx = pool.begin().await.unwrap();
                assert!(request.read_receipt(&mut tx).await.unwrap().is_none());
                tx.rollback().await.unwrap();
                sqlx::query("DROP TRIGGER reject_enabled_phase")
                    .execute(&pool)
                    .await
                    .unwrap();
            } else {
                let result = result.unwrap();
                request
                    .record_receipt(&mut tx, &receipt, now.epoch_ms)
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
                assert_eq!(result.run.as_ref().unwrap().occurrence_id, "source");
                assert!(
                    request
                        .start_focus_command(occurrence, &result)
                        .unwrap_err()
                        .contains("Another Focus run")
                );
                let mut tx = pool.begin().await.unwrap();
                assert_eq!(
                    request
                        .read_receipt(&mut tx)
                        .await
                        .unwrap()
                        .unwrap()
                        .edited_id,
                    "source"
                );
                let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
                assert_eq!(runs, 2);
                let original: (String, String) = sqlx::query_as(
                    "SELECT original_event_id, ended_at FROM pomodoro_runs WHERE id = 'run-1'",
                )
                .fetch_one(&mut *tx)
                .await
                .unwrap();
                assert_eq!(original, ("source".into(), "2099-05-10T10:40:00Z".into()));
                tx.rollback().await.unwrap();
            }
        }
    });
}

#[test]
fn end_now_detach_projects_earlier_closed_history_without_changing_execution_facts() {
    use crate::events::commit::check_review;
    use crate::events::edit::{EditAction, prepare_action};
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        insert_test_completed_pomodoro_history(&pool, "source", "source::2099-05-10", "2099-05-10")
            .await;
        let now = clock("2099-05-10T10:45:00Z");
        let mut tx = pool.begin().await.unwrap();
        let backwards = prepare_action(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            clock("2099-05-10T10:30:00Z"),
            EventDraft::default(),
            EditAction::EndNow,
        )
        .err()
        .unwrap();
        assert!(
            backwards.contains("precedes recorded Focus completion"),
            "{backwards}"
        );
        let prepared = prepare_action(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            now,
            EventDraft::default(),
            EditAction::EndNow,
        )
        .unwrap();
        let revision = prepared.review_revision.clone();
        let prepared = check_review(prepared, now, "end-after-run", &revision).unwrap();
        assert!(prepared.start_occurrence.is_none());
        let receipt = prepared.write(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        let original: (String, String, String) = sqlx::query_as("SELECT original_event_id, ended_at, current_occurrence_id FROM pomodoro_runs WHERE id = 'run-1'").fetch_one(&pool).await.unwrap();
        assert_eq!(
            original,
            (
                "source::2099-05-10".into(),
                "2099-05-10T10:40:00Z".into(),
                receipt.edited_id.clone()
            )
        );
        let segment: (String, String) = sqlx::query_as(
            "SELECT event_id, actual_end FROM pomodoro_segments WHERE id = 'segment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(segment, ("source".into(), "2099-05-10T10:40:00Z".into()));
        let visible =
            ganbaru_pomodoro::pomodoro_load_segments_for_events(pool, vec![receipt.edited_id])
                .await
                .unwrap();
        assert_eq!(visible.len(), 1);
    });
}

#[test]
fn enable_focus_rejects_series_existing_configuration_and_ineligible_geometry() {
    use crate::events::edit::{EditAction, prepare_action};
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let draft = json!({"pomodoroConfig":{"action":"set","value":sequence_pomodoro_config()}});
        let mut tx = pool.begin().await.unwrap();
        for sql in [
            "SELECT 1",
            "DELETE FROM calendar_event_pomodoro_configs WHERE event_id = 'source'",
        ] {
            sqlx::query(sql).execute(&mut *tx).await.unwrap();
            assert!(
                prepare_action(
                    read_snapshot(&mut tx, "source").await.unwrap(),
                    parse_date("2099-05-10").unwrap(),
                    EditScope::This,
                    clock("2099-05-10T10:45:00Z"),
                    serde_json::from_value(draft.clone()).unwrap(),
                    EditAction::EnableFocus
                )
                .is_err()
            );
        }
        sqlx::query("UPDATE calendar_events SET start_time = '2099-05-10T10:00:00Z', end_time = '2099-05-10T11:00:00Z', rrule = NULL WHERE id = 'source'").execute(&mut *tx).await.unwrap();
        for (instant, mut value) in [
            ("2099-05-10T09:59:00Z", draft.clone()),
            ("2099-05-10T11:00:00Z", draft.clone()),
            ("2099-05-10T10:45:00Z", json!({})),
            (
                "2099-05-10T10:45:00Z",
                json!({"timing":{"endTime":"2099-05-10T10:44:00Z"}}),
            ),
            (
                "2099-05-10T10:45:00Z",
                json!({"timing":{"startTime":"2099-05-10T10:01:00Z"}}),
            ),
            (
                "2099-05-10T10:45:00Z",
                json!({"recurrence":{"kind":"set","value":"FREQ=DAILY;COUNT=2"}}),
            ),
        ] {
            if value.get("timing").is_some() || value.get("recurrence").is_some() {
                value["pomodoroConfig"] = draft["pomodoroConfig"].clone();
            }
            assert!(
                prepare_action(
                    read_snapshot(&mut tx, "source").await.unwrap(),
                    parse_date("2099-05-10").unwrap(),
                    EditScope::This,
                    clock(instant),
                    serde_json::from_value(value).unwrap(),
                    EditAction::EnableFocus
                )
                .is_err(),
                "{instant}"
            );
        }
        tx.rollback().await.unwrap();
    });
}

async fn review(
    pool: &sqlx::SqlitePool,
    draft: Value,
    scope: EditScope,
    now: ScopeClock,
) -> (CommitRequest, String) {
    let mut tx = pool.begin().await.unwrap();
    let prepared = prepare(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        scope,
        now,
        serde_json::from_value(draft.clone()).unwrap(),
    )
    .unwrap();
    let revision = prepared.review_revision;
    tx.commit().await.unwrap();
    let request = serde_json::from_value(json!({
        "vaultId": "vault", "vaultGeneration": 1, "commandId": "save-1", "reviewRevision": revision,
        "edit": {"selection": {"templateId":"source", "recurrenceDate":"2099-05-10", "scope":scope}, "draft":draft}
    })).unwrap();
    (request, revision)
}

#[test]
fn following_save_copies_complete_metadata_and_replays_after_source_changes() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Edited series"}]});
        let (request, revision) = review(&pool, draft.clone(), EditScope::Following, now).await;
        let mut tx = pool.begin().await.unwrap();
        assert!(request.read_receipt(&mut tx).await.unwrap().is_none());
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::Following,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        let receipt = prepared.write(&mut tx).await.unwrap();
        assert!(receipt.changed);
        assert_ne!(receipt.edited_id, "source");
        request
            .record_receipt(&mut tx, &receipt, now.epoch_ms)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        for (table, owner, expected) in [
            ("calendar_event_attendees", "event_id", 1),
            ("calendar_event_alarms", "event_id", 1),
            ("calendar_event_categories", "event_id", 1),
            ("calendar_event_extended_properties", "event_id", 1),
            ("calendar_event_notifications", "event_id", 1),
            ("calendar_event_organizers", "event_id", 1),
            ("calendar_event_overrides", "parent_event_id", 1),
            ("calendar_event_pomodoro_configs", "event_id", 1),
            (
                "calendar_event_pomodoro_config_sequence_steps",
                "event_id",
                1,
            ),
            ("music_context_assignments", "owner_id", 1),
            ("project_task_event_links", "event_id", 1),
        ] {
            let count: i64 =
                sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE {owner} = ?"))
                    .bind(&receipt.edited_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(count, expected, "{table}");
        }
        let override_data: (String, String, String) = sqlx::query_as("SELECT o.title, p.property_value, c.projected_id FROM calendar_event_overrides o JOIN calendar_event_override_extended_properties p ON p.override_id = o.id JOIN icalendar_components c ON c.id = o.icalendar_component_id WHERE o.parent_event_id = ?")
            .bind(&receipt.edited_id).fetch_one(&pool).await.unwrap();
        assert_eq!(override_data.0, "Different");
        assert_eq!(override_data.1, "override data");
        assert_ne!(override_data.2, "override");
        let mut tx = pool.begin().await.unwrap();
        let source = read_snapshot(&mut tx, "source")
            .await
            .unwrap()
            .geometry
            .template(true)
            .unwrap();
        let edited = read_snapshot(&mut tx, &receipt.edited_id)
            .await
            .unwrap()
            .geometry
            .template(true)
            .unwrap();
        for (date, before, after) in [
            ("2099-05-09", true, false),
            ("2099-05-10", false, true),
            ("2099-05-11", false, true),
            ("2099-05-12", false, false),
        ] {
            let date = Some(parse_date(date).unwrap());
            assert_eq!(source.resolve_identity(date).unwrap().is_some(), before);
            assert_eq!(edited.resolve_identity(date).unwrap().is_some(), after);
        }
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM calendar_events WHERE id = 'source'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM icalendar_objects WHERE id = 'object'")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let retry = request.read_receipt(&mut tx).await.unwrap().unwrap();
        assert_eq!(retry.edited_id, receipt.edited_id);
        let preserved: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events e JOIN icalendar_component_properties p ON p.component_id = e.icalendar_component_id WHERE e.id = ? AND p.name = 'x-preserved'")
            .bind(&receipt.edited_id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(preserved, 1);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn semantic_save_failure_rolls_back_the_source_copy_and_receipt() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Edited series"}]});
        let (request, revision) = review(&pool, draft.clone(), EditScope::Following, now).await;
        sqlx::query("CREATE TRIGGER reject_edited_notification BEFORE INSERT ON calendar_event_notifications WHEN NEW.event_id NOT IN ('source', 'target') BEGIN SELECT RAISE(ABORT, 'injected late child failure'); END")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::Following,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        let error = prepared.write(&mut tx).await.err().unwrap();
        assert!(error.contains("injected late child failure"), "{error}");
        tx.rollback().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(request.read_receipt(&mut tx).await.unwrap().is_none());
        let source: String =
            sqlx::query_scalar("SELECT rrule FROM calendar_events WHERE id = 'source'")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(source, "FREQ=DAILY;COUNT=3");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(count, 2);
        let objects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(objects, 1);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn unchanged_metadata_does_not_split_or_replace_imported_children() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":""},{"field":"categories","value":"[\"Work\"]"},{"field":"notifications","value":"[12]"}]});
        let (request, revision) = review(&pool, draft.clone(), EditScope::Following, now).await;
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::Following,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        let receipt = prepared.write(&mut tx).await.unwrap();
        assert!(!receipt.changed);
        assert_eq!(receipt.edited_id, "source");
        request
            .record_receipt(&mut tx, &receipt, now.epoch_ms)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let row: (String, i64) = sqlx::query_as(
            "SELECT id, sort_order FROM calendar_event_categories WHERE event_id = 'source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("category".into(), 7));
    });
}

#[test]
fn child_changes_and_new_started_protection_invalidate_the_review() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Edited"}]});
        let (_, revision) = review(&pool, draft.clone(), EditScope::All, now).await;
        for mutate_child in [false, true] {
            let mut tx = pool.begin().await.unwrap();
            if mutate_child {
                sqlx::query("UPDATE calendar_event_notifications SET offset_minutes = 15 WHERE event_id = 'source'").execute(&mut *tx).await.unwrap();
            }
            let current = if mutate_child {
                now
            } else {
                clock("2099-05-10T10:30:00Z")
            };
            let error = prepare_rows(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::All,
                current,
                serde_json::from_value(draft.clone()).unwrap(),
                "save-1",
                &revision,
            )
            .err()
            .unwrap();
            assert!(error.contains("review the current edit"), "{error}");
            tx.rollback().await.unwrap();
        }
    });
}

#[test]
fn receipt_rejects_command_identity_reuse_with_different_intent() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Edited"}]});
        let (request, revision) = review(&pool, draft.clone(), EditScope::Following, now).await;
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::Following,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        let receipt = prepared.write(&mut tx).await.unwrap();
        request
            .record_receipt(&mut tx, &receipt, now.epoch_ms)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut value = serde_json::to_value(request).unwrap();
        value["edit"]["draft"]["fields"][0]["value"] = json!("Different");
        let different: CommitRequest = serde_json::from_value(value).unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error = different.read_receipt(&mut tx).await.err().unwrap();
        assert!(error.contains("different intent"), "{error}");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn explicit_clears_do_not_clear_omitted_configuration() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"notifications","value":null},{"field":"title","value":"Single"}],"alarms":[]});
        let (_, revision) = review(&pool, draft.clone(), EditScope::This, now).await;
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            now,
            serde_json::from_value::<EventDraft>(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        let receipt = prepared.write(&mut tx).await.unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_notifications WHERE event_id = ?",
        )
        .bind(&receipt.edited_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(count, 0);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_alarms WHERE event_id = ?")
                .bind(&receipt.edited_id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(count, 0);
        let rhythm: String = sqlx::query_scalar(
            "SELECT rhythm_kind FROM calendar_event_pomodoro_configs WHERE event_id = ?",
        )
        .bind(&receipt.edited_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(rhythm, "sequence");
        let override_extension: String = sqlx::query_scalar("SELECT property_value FROM calendar_event_extended_properties WHERE event_id = ? AND property_key = 'X-EXTRA'").bind(&receipt.edited_id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(override_extension, "override data");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn materialized_history_keeps_original_execution_and_projects_its_current_calendar_owner() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        insert_test_completed_pomodoro_history(&pool, "source", "source::2099-05-11", "2099-05-10")
            .await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Future title"}]});
        let (_, revision) = review(&pool, draft.clone(), EditScope::All, now).await;
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        prepared.write(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        let run: (String, String, String, String) = sqlx::query_as("SELECT event_id, original_event_id, event_date, current_occurrence_id FROM pomodoro_runs").fetch_one(&pool).await.unwrap();
        assert_eq!(run.1, "source::2099-05-11");
        assert_eq!(run.2, "2099-05-10");
        assert_eq!(run.0, run.3);
        assert_ne!(run.0, "source");
        let physical: (String, String) =
            sqlx::query_as("SELECT event_id, event_date FROM pomodoro_segments")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(physical, ("source".into(), "2099-05-10".into()));
        let projected =
            ganbaru_pomodoro::pomodoro_load_segments_for_events(pool.clone(), vec![run.0.clone()])
                .await
                .unwrap();
        assert_eq!(projected.len(), 1);
        let projected = serde_json::to_value(&projected[0]).unwrap();
        assert_eq!(projected["event_id"], run.0);
        assert_eq!(projected["event_date"], "2099-05-10");
        let old = ganbaru_pomodoro::pomodoro_load_segments_for_events(pool, vec!["source".into()])
            .await
            .unwrap();
        assert!(old.is_empty());
    });
}

#[test]
fn active_detach_and_recurrence_creation_share_atomic_focus_identity_and_receipts() {
    use ganbaru_pomodoro::*;
    crate::test_support::block_on(async {
        for recurring in [false, true] {
            let pool = super::metadata::seed().await;
            if !recurring {
                sqlx::query("UPDATE calendar_events SET start_time = '2099-05-10T10:00:00Z', end_time = '2099-05-10T11:00:00Z', rrule = NULL WHERE id = 'source'").execute(&pool).await.unwrap();
            }
            let original_id = if recurring {
                "source::2099-05-10"
            } else {
                "source"
            };
            let now = clock("2099-05-10T10:05:00Z");
            let mut tx = pool.begin().await.unwrap();
            let calendar =
                crate::reads::focus_context::resolve(&mut tx, now.epoch_ms, Some(original_id))
                    .await
                    .unwrap();
            let context = FocusExecutionContext {
                now_ms: now.epoch_ms,
                platform: FocusPlatform::Desktop,
                foreground: true,
                commitment: calendar.commitment,
                planned_blocks: Vec::new(),
                local_time: None,
            };
            let before = focus_execute_command_tx(
                &mut tx,
                &FocusCommand {
                    command_id: "start".into(),
                    expected_revision: 0,
                    intent: FocusIntent::StartScheduled {
                        occurrence_id: Some(original_id.into()),
                    },
                },
                &context,
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
            let mut draft = json!({"fields":[{"field":"title","value":"Active edit"}],"timing":{"endTime":"2099-05-10T11:10:00Z"}});
            if !recurring {
                draft["recurrence"] = json!({"kind":"set","value":"FREQ=DAILY;COUNT=2"});
            }
            let (request, revision) = review(&pool, draft.clone(), EditScope::All, now).await;
            for inject_failure in [true, false] {
                let mut tx = pool.begin().await.unwrap();
                let prepared = prepare_rows(
                    read_snapshot(&mut tx, "source").await.unwrap(),
                    parse_date("2099-05-10").unwrap(),
                    EditScope::All,
                    now,
                    serde_json::from_value(draft.clone()).unwrap(),
                    "save-1",
                    &revision,
                )
                .unwrap();
                let mut change = prepared.active_change(&before).unwrap().unwrap();
                let receipt = prepared.write(&mut tx).await.unwrap();
                let calendar = crate::reads::focus_context::resolve(
                    &mut tx,
                    now.epoch_ms,
                    Some(&receipt.edited_id),
                )
                .await
                .unwrap();
                let commitment = calendar.commitment.unwrap();
                change.target_event_date.clone_from(&commitment.event_date);
                let context = FocusExecutionContext {
                    now_ms: now.epoch_ms,
                    platform: FocusPlatform::Desktop,
                    foreground: true,
                    commitment: Some(commitment),
                    planned_blocks: Vec::new(),
                    local_time: None,
                };
                if inject_failure {
                    sqlx::query("CREATE TEMP TRIGGER fail_reference BEFORE UPDATE OF event_id ON pomodoro_segments BEGIN SELECT RAISE(ABORT, 'injected phase reference failure'); END").execute(&mut *tx).await.unwrap();
                }
                let result =
                    focus_retarget_calendar_tx(&mut tx, before.revision, &change, &context).await;
                if inject_failure {
                    assert!(
                        result
                            .err()
                            .unwrap()
                            .message
                            .contains("injected phase reference failure")
                    );
                    tx.rollback().await.unwrap();
                    let current = focus_read_execution_snapshot(&pool, now.epoch_ms)
                        .await
                        .unwrap();
                    assert_eq!(current.revision, before.revision);
                    assert_eq!(current.run.unwrap().occurrence_id, original_id);
                } else {
                    let result = result.unwrap();
                    request
                        .record_receipt(&mut tx, &receipt, now.epoch_ms)
                        .await
                        .unwrap();
                    tx.commit().await.unwrap();
                    let current = focus_read_execution_snapshot(&pool, now.epoch_ms)
                        .await
                        .unwrap();
                    assert_eq!(
                        current.run.as_ref().unwrap().occurrence_id,
                        receipt.edited_id
                    );
                    assert_eq!(
                        current.segment.as_ref().unwrap().id,
                        before.segment.as_ref().unwrap().id
                    );
                    assert_eq!(current.revision, result.revision);
                    let original: String = sqlx::query_scalar(
                        "SELECT original_event_id FROM pomodoro_runs WHERE id = ?",
                    )
                    .bind(&before.run.as_ref().unwrap().id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                    assert_eq!(original, original_id);
                }
            }
        }
    });
}

#[test]
fn unchanged_selected_override_metadata_does_not_detach_its_instance() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Different"}]});
        let (_, revision) = review(&pool, draft.clone(), EditScope::This, now).await;
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .unwrap();
        let receipt = prepared.write(&mut tx).await.unwrap();
        assert!(!receipt.changed);
        assert_eq!(receipt.edited_id, "source");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn large_preserved_metadata_and_history_fanout_fail_before_any_writes() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::query("UPDATE calendar_events SET rrule = 'FREQ=DAILY;COUNT=30' WHERE id = 'source'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE calendar_event_extended_properties SET property_value = ? WHERE event_id = 'source'")
            .bind("x".repeat(950_000)).execute(&pool).await.unwrap();
        for day in 11..=28 {
            let date = format!("2099-05-{day}");
            insert_completed_history_with_ids(
                &pool,
                "source",
                &format!("source::{date}"),
                &date,
                &format!("run-{day}"),
                &format!("segment-{day}"),
            )
            .await;
        }
        let now = clock("2099-05-08T00:00:00Z");
        let draft = json!({"fields":[{"field":"title","value":"Changed"}]});
        let (_, revision) = review(&pool, draft.clone(), EditScope::All, now).await;
        let mut tx = pool.begin().await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let error = prepare_rows(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            now,
            serde_json::from_value(draft).unwrap(),
            "save-1",
            &revision,
        )
        .err()
        .unwrap();
        assert!(error.contains("fanout"), "{error}");
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(before, after);
        tx.rollback().await.unwrap();
    });
}
