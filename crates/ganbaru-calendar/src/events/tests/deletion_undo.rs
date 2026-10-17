use super::fixtures::*;
use crate::events::commit::CommitRequest;
use crate::events::deletion::{UndoPreimage, prepare, prepare_commit};
use crate::events::metadata::Metadata;
use crate::events::occurrence::ReadBudget;
use crate::events::scope::read_snapshot;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use serde_json::json;
use std::sync::Arc;

fn clock() -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339("2099-05-08T12:00:00Z")
            .unwrap()
            .timestamp_millis(),
        floating_today: None,
    }
}

async fn deleted(pool: &sqlx::SqlitePool) -> Arc<UndoPreimage> {
    let mut tx = pool.begin().await.unwrap();
    let clock = clock();
    let review = prepare(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        EditScope::All,
        clock,
    )
    .unwrap();
    let request: CommitRequest = serde_json::from_value(json!({
        "vaultId":"vault", "vaultGeneration":1, "commandId":"delete", "reviewRevision":review.review_revision,
        "edit":{"kind":"delete", "selection":{"templateId":"source", "recurrenceDate":"2099-05-10", "scope":"all"}, "stopActive":false}
    })).unwrap();
    let rows = prepare_commit(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        EditScope::All,
        clock,
        "delete",
        &review.review_revision,
        false,
    )
    .unwrap();
    let (rows, undo) = rows.into_write();
    let mut undo = undo.unwrap();
    let receipt = rows.write(&mut tx).await.unwrap();
    assert_eq!(
        receipt.undo_review_revision.as_deref(),
        Some(undo.review_revision.as_str())
    );
    request
        .record_receipt(&mut tx, &receipt, clock.epoch_ms)
        .await
        .unwrap();
    undo.seal(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    Arc::new(undo)
}

fn undo_request(undo: &UndoPreimage) -> CommitRequest {
    serde_json::from_value(json!({
        "vaultId":"vault", "vaultGeneration":1, "commandId":"undo", "reviewRevision":undo.review_revision,
        "edit":{"kind":"undo_delete", "deleteCommandId":"delete"}
    })).unwrap()
}

#[test]
fn native_undo_restores_complete_original_rows_and_releases_only_its_archive_copies() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        let before = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .revision()
            .unwrap();
        tx.rollback().await.unwrap();
        let undo = deleted(&pool).await;
        let request = undo_request(&undo);
        let mut tx = pool.begin().await.unwrap();
        assert!(request.read_receipt(&mut tx).await.unwrap().is_none());
        let rows = undo.clone().prepare(&mut tx, "undo").await.unwrap();
        let receipt = rows.write(&mut tx).await.unwrap();
        request
            .record_receipt(&mut tx, &receipt, clock().epoch_ms)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let after = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .revision()
            .unwrap();
        assert_eq!(after, before);
        let remaining: (i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM calendar_event_archives), (SELECT COUNT(*) FROM icalendar_objects)")
            .fetch_one(&mut *tx).await.unwrap();
        assert_eq!(remaining, (0, 1));
        tx.rollback().await.unwrap();
        // Accepted Undo is durable even after process-local preimage removal.
        drop(undo);
        sqlx::query("DELETE FROM calendar_events WHERE id='source'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(request.read_only_retry(&pool).await.unwrap().is_some());
    });
}

#[test]
fn hard_deleted_future_source_has_only_a_process_local_preimage_and_restores_its_configuration() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "source",
            "2099-05-10T10:00:00Z",
            "2099-05-10T11:00:00Z",
            None,
        )
        .await;
        sqlx::raw_sql("UPDATE calendar_events SET title='Ephemeral future title' WHERE id='source';
            INSERT INTO calendar_event_pomodoro_configs (event_id, rhythm_kind, rhythm_source) VALUES ('source', 'sequence', 'custom');
            INSERT INTO calendar_event_pomodoro_config_sequence_steps (event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes)
                VALUES ('source', 0, 25, 'short_break', 5);
            INSERT INTO music_context_assignments (owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms, version)
                VALUES ('event-snapshot', 'source', 'focus', 'pause-music', 'explicit', 8, 4);")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let before = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .revision()
            .unwrap();
        tx.rollback().await.unwrap();
        let undo = deleted(&pool).await;
        let remaining: (i64, i64, String) = sqlx::query_as(
            "SELECT
            (SELECT COUNT(*) FROM calendar_events WHERE id='source'),
            (SELECT COUNT(*) FROM calendar_event_archives),
            (SELECT result_json FROM calendar_edit_receipts WHERE command_id='delete')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((remaining.0, remaining.1), (0, 0));
        assert!(!remaining.2.contains("Ephemeral future title"));
        let mut tx = pool.begin().await.unwrap();
        undo.prepare(&mut tx, "undo")
            .await
            .unwrap()
            .write(&mut tx)
            .await
            .unwrap();
        let after = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .revision()
            .unwrap();
        assert_eq!(after, before);
        tx.commit().await.unwrap();
    });
}

#[test]
fn calendar_undo_restores_active_run_projections_without_restarting_or_rewinding_execution() {
    use ganbaru_pomodoro::*;
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let started = ScopeClock {
            epoch_ms: chrono::DateTime::parse_from_rfc3339("2099-05-10T10:05:00Z")
                .unwrap()
                .timestamp_millis(),
            floating_today: None,
        };
        let mut tx = pool.begin().await.unwrap();
        let before_metadata = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .revision()
            .unwrap();
        let calendar = crate::reads::focus_context::resolve(
            &mut tx,
            started.epoch_ms,
            Some("source::2099-05-10"),
        )
        .await
        .unwrap();
        let context = FocusExecutionContext {
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
                command_id: "start".into(),
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
        let review = prepare(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            started,
        )
        .unwrap();
        let request: CommitRequest = serde_json::from_value(json!({
            "vaultId":"vault", "vaultGeneration":1, "commandId":"delete", "reviewRevision":review.review_revision,
            "edit":{"kind":"delete", "selection":{"templateId":"source", "recurrenceDate":"2099-05-10", "scope":"all"}, "stopActive":true}
        })).unwrap();
        let rows = prepare_commit(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            started,
            "delete",
            &review.review_revision,
            true,
        )
        .unwrap();
        let command = rows.stop_focus_command(&request, &before).unwrap().unwrap();
        focus_execute_command_tx(&mut tx, &command, &context)
            .await
            .unwrap();
        let (rows, undo) = rows.into_write();
        let mut undo = undo.unwrap();
        let receipt = rows.write(&mut tx).await.unwrap();
        request
            .record_receipt(&mut tx, &receipt, started.epoch_ms)
            .await
            .unwrap();
        undo.seal(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        let facts_sql = "SELECT r.original_event_id, r.event_date, r.planned_start, r.planned_end, r.started_at, r.ended_at, r.end_reason,
            s.actual_start, s.actual_end, s.status, s.end_reason FROM pomodoro_runs r JOIN pomodoro_segments s ON s.run_id=r.id WHERE r.id=?";
        let run_id = before.run.as_ref().unwrap().id.clone();
        let stopped_facts: (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = sqlx::query_as(facts_sql)
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let undo = Arc::new(undo);
        let mut tx = pool.begin().await.unwrap();
        undo.prepare(&mut tx, "undo")
            .await
            .unwrap()
            .write(&mut tx)
            .await
            .unwrap();
        let calendar = crate::reads::focus_context::resolve(
            &mut tx,
            started.epoch_ms,
            Some("source::2099-05-10"),
        )
        .await
        .unwrap();
        let context = FocusExecutionContext {
            commitment: calendar.commitment,
            ..context
        };
        let after =
            focus_apply_observation_tx(&mut tx, &FocusObservation::CalendarChanged, &context)
                .await
                .unwrap();
        assert_eq!(after.mode, FocusMode::Stopped);
        assert_eq!(after.run.as_ref().unwrap().id, run_id);
        let restored_metadata = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .revision()
            .unwrap();
        assert_eq!(restored_metadata, before_metadata);
        tx.commit().await.unwrap();
        let restored_facts: (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = sqlx::query_as(facts_sql)
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(restored_facts, stopped_facts);
        let refs: (String, Option<String>, String) = sqlx::query_as("SELECT r.event_id, r.calendar_archive_id, s.event_id FROM pomodoro_runs r JOIN pomodoro_segments s ON s.run_id=r.id WHERE r.id=?")
            .bind(run_id).fetch_one(&pool).await.unwrap();
        assert_eq!(refs, ("source".into(), None, "source".into()));
    });
}

#[test]
fn native_undo_rejects_recreated_sources_and_changed_original_import_graphs() {
    crate::test_support::block_on(async {
        for recreated in [false, true] {
            let pool = super::metadata::seed().await;
            let undo = deleted(&pool).await;
            if recreated {
                insert_test_event_at(
                    &pool,
                    "source",
                    "2099-05-09T10:00:00Z",
                    "2099-05-09T11:00:00Z",
                    None,
                )
                .await;
            } else {
                sqlx::query("UPDATE icalendar_value_nodes SET text_value='Later shared edit' WHERE id='parameter-value'")
                    .execute(&pool).await.unwrap();
            }
            let mut tx = pool.begin().await.unwrap();
            let before: i64 = sqlx::query_scalar("SELECT total_changes()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            let failure = undo.prepare(&mut tx, "undo").await.err().unwrap();
            assert!(failure.contains("changed after deletion"), "{failure}");
            let after: i64 = sqlx::query_scalar("SELECT total_changes()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            assert_eq!(before, after);
            tx.rollback().await.unwrap();
            let archives: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_archives")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(archives, 1);
        }
    });
}

#[test]
fn late_undo_failure_rolls_back_restored_metadata_references_and_archive_cleanup() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let undo = deleted(&pool).await;
        let request = undo_request(&undo);
        sqlx::query("CREATE TRIGGER reject_undo BEFORE INSERT ON calendar_edit_receipts WHEN NEW.command_id='undo' BEGIN SELECT RAISE(ABORT, 'undo receipt failed'); END")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let rows = undo.clone().prepare(&mut tx, "undo").await.unwrap();
        let receipt = rows.write(&mut tx).await.unwrap();
        assert!(
            request
                .record_receipt(&mut tx, &receipt, clock().epoch_ms)
                .await
                .unwrap_err()
                .contains("undo receipt failed")
        );
        tx.rollback().await.unwrap();
        let remaining: (i64, i64, i64) = sqlx::query_as("SELECT
            (SELECT COUNT(*) FROM calendar_events WHERE id='source'),
            (SELECT COUNT(*) FROM calendar_event_archives), (SELECT COUNT(*) FROM icalendar_objects)")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(remaining, (0, 1, 2));
        sqlx::query("DROP TRIGGER reject_undo")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        undo.prepare(&mut tx, "undo")
            .await
            .unwrap()
            .write(&mut tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    });
}
