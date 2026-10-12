use crate::events::commit::CommitRequest;
use crate::events::task_schedule::ScheduleRequest;
use crate::recurrence::canonical::Window;
use serde_json::{Value, json};

fn request(start: &str, zone: &str) -> ScheduleRequest {
    serde_json::from_value(json!({"kind":"schedule_tasks", "projectId":"project",
        "tasks":[{"id":"task", "revision":0}, {"id":"task-2", "revision":0}],
        "startTime":start, "timezone":zone, "durationMinutes":30, "globalIdleTimeoutMinutes":5}))
    .unwrap()
}

async fn seed() -> sqlx::SqlitePool {
    let pool = super::metadata::seed().await;
    sqlx::raw_sql("INSERT INTO project_tasks (id, project_id, section_id, status_id, title, completed_at, due_date)
        VALUES ('task-2', 'project', 'section', 'status', 'Second', '2026-01-01T09:00:00Z', '2099-12-01');
        UPDATE projects SET default_event_duration_minutes = 120, color = 3;")
        .execute(&pool).await.unwrap();
    pool
}

fn command(request: &ScheduleRequest, revision: &str) -> CommitRequest {
    serde_json::from_value(
        json!({"vaultId":"vault", "vaultGeneration":1, "commandId":"schedule-1",
        "reviewRevision":revision, "edit":request}),
    )
    .unwrap()
}

#[test]
fn scheduling_commits_selected_duration_links_dates_history_and_focus_configuration_together() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let request = request("2099-05-10T09:00", "UTC");
        let mut tx = pool.begin().await.unwrap();
        let prepared = request
            .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 1_000)
            .unwrap();
        let window = Window::new(
            "2099-05-10",
            "2099-05-10",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let preview =
            serde_json::to_value(prepared.project("schedule-1", &window).unwrap()).unwrap();
        assert_eq!(
            preview["window"]["occurrences"].as_array().unwrap().len(),
            2
        );
        let commit = command(&request, preview["reviewRevision"].as_str().unwrap());
        let prepared = request
            .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 2_000)
            .unwrap();
        let receipt = prepared
            .into_commit("schedule-1")
            .write(&mut tx)
            .await
            .unwrap();
        commit
            .record_receipt(&mut tx, &receipt, 2_000)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let rows: Vec<(String, String, String, String)> = sqlx::query_as("SELECT t.id, e.title, e.start_time, e.end_time FROM project_task_event_links l
            JOIN project_tasks t ON t.id=l.task_id JOIN calendar_events e ON e.id=l.event_id WHERE e.id LIKE 'calendar-schedule-%' ORDER BY t.id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].2, "2099-05-10T09:00:00.000Z");
        assert_eq!(rows[0].3, "2099-05-10T09:30:00.000Z");
        assert_eq!(rows[1].2, rows[0].3);
        let facts: (String, String, String, String, i64) = sqlx::query_as("SELECT start_date, target_end_date, due_date, completed_at, revision FROM project_tasks WHERE id='task-2'").fetch_one(&pool).await.unwrap();
        assert_eq!(
            facts,
            (
                "2099-05-10".into(),
                "2099-05-10".into(),
                "2099-12-01".into(),
                "2026-01-01T09:00:00Z".into(),
                1
            )
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_pomodoro_configs WHERE event_id LIKE 'calendar-schedule-%' AND idle_timeout_minutes=5").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 2);
        let histories: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_task_change_events WHERE task_id IN ('task','task-2')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(histories, 7);
        // Canonical retry is read-only and does not reread now-stale task revisions.
        sqlx::query("DELETE FROM calendar_events WHERE id LIKE 'calendar-schedule-%'")
            .execute(&pool)
            .await
            .unwrap();
        let mut restarted = serde_json::to_value(&commit).unwrap();
        restarted["vaultGeneration"] = json!(42);
        let restarted: CommitRequest = serde_json::from_value(restarted).unwrap();
        sqlx::query("PRAGMA query_only=ON")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(restarted.read_only_retry(&pool).await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(receipt).unwrap()
        );
    });
}

#[test]
fn scheduling_rolls_back_every_event_task_link_date_history_and_receipt_after_late_failure() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        sqlx::raw_sql("CREATE TRIGGER reject_second_schedule BEFORE INSERT ON project_task_event_links
            WHEN NEW.task_id='task-2' BEGIN SELECT RAISE(ABORT, 'injected scheduling failure'); END;").execute(&pool).await.unwrap();
        let request = request("2099-05-10T09:00", "UTC");
        let mut tx = pool.begin().await.unwrap();
        let prepared = request
            .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 1_000)
            .unwrap();
        assert!(
            prepared
                .into_commit("schedule-1")
                .write(&mut tx)
                .await
                .err()
                .unwrap()
                .contains("injected scheduling failure")
        );
        tx.rollback().await.unwrap();
        for table in ["calendar_events", "calendar_event_pomodoro_configs"] {
            let count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {table} WHERE {} LIKE 'calendar-schedule-%'",
                if table == "calendar_events" {
                    "id"
                } else {
                    "event_id"
                }
            ))
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(count, 0);
        }
        let tasks: Vec<(i64, Option<String>)> =
            sqlx::query_as("SELECT revision, start_date FROM project_tasks ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(tasks, vec![(0, None), (0, None)]);
        let histories: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_task_change_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(histories, 0);
        let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_edit_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
        sqlx::query("DROP TRIGGER reject_second_schedule")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let prepared = request
            .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 2_000)
            .unwrap();
        prepared
            .into_commit("schedule-1")
            .write(&mut tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    });
}

#[test]
fn scheduling_rejects_stale_archived_foreign_duplicate_and_oversized_selections() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let base = serde_json::to_value(request("2099-05-10T09:00", "UTC")).unwrap();
        sqlx::raw_sql("INSERT INTO projects (id, group_id, name) VALUES ('foreign', 'group', 'Foreign');
            INSERT INTO project_sections (id, project_id, name) VALUES ('foreign-section', 'foreign', 'General');
            INSERT INTO project_statuses (id, project_id, name, category) VALUES ('foreign-status', 'foreign', 'Open', 'not_started');
            INSERT INTO project_tasks (id, project_id, section_id, status_id, title)
                VALUES ('foreign-task', 'foreign', 'foreign-section', 'foreign-status', 'Foreign task');")
            .execute(&pool).await.unwrap();
        for patch in [
            json!([{"id":"task","revision":1}]),
            json!([{"id":"task","revision":0},{"id":"task","revision":0}]),
            json!([{"id":"missing","revision":0}]),
            json!([{"id":"foreign-task","revision":0}]),
        ] {
            let mut bad = base.clone();
            bad["tasks"] = patch;
            let bad: ScheduleRequest = serde_json::from_value(bad).unwrap();
            let mut tx = pool.begin().await.unwrap();
            assert!(bad.read(&mut tx).await.is_err());
            tx.rollback().await.unwrap();
        }
        sqlx::query("UPDATE project_tasks SET archived_at='now' WHERE id='task'")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            request("2099-05-10T09:00", "UTC")
                .read(&mut tx)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        let mut oversized = base;
        let mut oversized_rows = oversized.clone();
        oversized_rows["tasks"] = Value::Array(
            (0..1_001)
                .map(|index| json!({"id":format!("task-{index}"),"revision":0}))
                .collect(),
        );
        assert!(
            serde_json::from_value::<ScheduleRequest>(oversized_rows)
                .unwrap()
                .check_limits()
                .is_err()
        );
        oversized["durationMinutes"] = json!(1441);
        assert!(
            serde_json::from_value::<ScheduleRequest>(oversized)
                .unwrap()
                .check_limits()
                .is_err()
        );
    });
}

#[test]
fn scheduling_reviews_project_defaults_and_uses_elapsed_dst_intervals() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        for (start, expected_end) in [
            ("2026-03-08T01:45", "2026-03-08T07:15:00.000Z"),
            ("2026-11-01T01:45", "2026-11-01T06:15:00.000Z"),
        ] {
            let request = request(start, "America/New_York");
            let mut tx = pool.begin().await.unwrap();
            let prepared = request
                .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 1_000)
                .unwrap();
            let date = &start[..10];
            let window = Window::new(
                date,
                date,
                &ganbaru_civil_time::zone("America/New_York").unwrap(),
            )
            .unwrap();
            let preview =
                serde_json::to_value(prepared.project("schedule-1", &window).unwrap()).unwrap();
            let occurrences = preview["window"]["occurrences"].as_array().unwrap();
            assert_eq!(occurrences.len(), 2);
            for occurrence in occurrences {
                let start = chrono::DateTime::parse_from_rfc3339(
                    occurrence["start_time"].as_str().unwrap(),
                )
                .unwrap();
                let end =
                    chrono::DateTime::parse_from_rfc3339(occurrence["end_time"].as_str().unwrap())
                        .unwrap();
                assert_eq!(
                    end.timestamp_millis() - start.timestamp_millis(),
                    30 * 60 * 1000
                );
            }
            assert_eq!(occurrences[0]["end_time"], expected_end);
            assert_eq!(occurrences[0]["end_time"], occurrences[1]["start_time"]);
            tx.rollback().await.unwrap();
        }
        let request = request("2099-05-10T09:00", "UTC");
        let mut tx = pool.begin().await.unwrap();
        let before = request
            .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 1_000)
            .unwrap();
        let window = Window::new(
            "2099-05-10",
            "2099-05-10",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let before: Value =
            serde_json::to_value(before.project("schedule-1", &window).unwrap()).unwrap();
        sqlx::query("UPDATE projects SET color=4, default_event_time_mode='all_day'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let after = request
            .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 2_000)
            .unwrap();
        let after = serde_json::to_value(after.project("schedule-1", &window).unwrap()).unwrap();
        assert_ne!(before["reviewRevision"], after["reviewRevision"]);
        let changed_snapshot = request.read(&mut tx).await.unwrap();
        assert!(
            request
                .prepare_reviewed(
                    changed_snapshot,
                    "schedule-1",
                    before["reviewRevision"].as_str().unwrap(),
                    3_000
                )
                .err()
                .unwrap()
                .contains("inputs changed")
        );
        for event in after["window"]["events"].as_array().unwrap() {
            assert_eq!(event["start_time"], "2099-05-10");
            assert_eq!(event["end_time"], "2099-05-10");
            assert!(event["rhythm_kind"].is_null());
        }
        tx.rollback().await.unwrap();
    });
}

#[test]
fn scheduling_snapshots_soundtrack_and_custom_idle_and_rejects_changed_review() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        sqlx::raw_sql("INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms) VALUES ('playlist', 'Focus', 1, 1);
            INSERT INTO music_context_assignments (owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, updated_at_ms)
                VALUES ('project-default', 'project', 'focus', 'play-automatically', 'playlist', 'explicit', 1);
            UPDATE projects SET default_pomodoro_mode='custom', default_pomodoro_preset_key=NULL, default_pomodoro_focus_minutes=35,
                default_pomodoro_short_break_minutes=7, default_pomodoro_long_break_minutes=12,
                default_pomodoro_long_break_after_focus_count=3, default_idle_settings_source='custom',
                default_idle_pause_enabled=1, default_idle_threshold_minutes=10;")
            .execute(&pool).await.unwrap();
        let request = request("2099-05-10T09:00", "UTC");
        let mut tx = pool.begin().await.unwrap();
        let window = Window::new(
            "2099-05-10",
            "2099-05-10",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let preview = serde_json::to_value(
            request
                .prepare(request.read(&mut tx).await.unwrap(), "schedule-1", 1_000)
                .unwrap()
                .project("schedule-1", &window)
                .unwrap(),
        )
        .unwrap();
        let review = preview["reviewRevision"].as_str().unwrap();
        sqlx::query("UPDATE music_context_assignments SET behavior='pause-music' WHERE owner_kind='project-default'")
            .execute(&mut *tx).await.unwrap();
        assert!(
            request
                .prepare_reviewed(
                    request.read(&mut tx).await.unwrap(),
                    "schedule-1",
                    review,
                    2_000
                )
                .is_err()
        );
        tx.rollback().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let prepared = request
            .prepare_reviewed(
                request.read(&mut tx).await.unwrap(),
                "schedule-1",
                review,
                2_000,
            )
            .unwrap();
        let receipt = prepared.write(&mut tx).await.unwrap();
        command(&request, review)
            .record_receipt(&mut tx, &receipt, 2_000)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let assignments: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT phase, behavior, provenance_kind, provenance_id
            FROM music_context_assignments WHERE owner_kind='event-snapshot' AND owner_id LIKE 'calendar-schedule-%' ORDER BY owner_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            assignments,
            vec![
                (
                    "focus".into(),
                    "play-automatically".into(),
                    "copied-project".into(),
                    "project".into()
                );
                2
            ]
        );
        let configs: Vec<(String, i64, i64)> = sqlx::query_as("SELECT c.rhythm_source, c.idle_timeout_minutes, r.focus_duration_minutes
            FROM calendar_event_pomodoro_configs c JOIN calendar_event_pomodoro_config_count_rhythms r ON r.event_id=c.event_id WHERE c.event_id LIKE 'calendar-schedule-%'")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(configs, vec![("custom".into(), 10, 35); 2]);
        sqlx::query("UPDATE music_context_assignments SET behavior='pause-music' WHERE owner_kind='project-default'").execute(&pool).await.unwrap();
        let retained: String = sqlx::query_scalar("SELECT behavior FROM music_context_assignments WHERE owner_kind='event-snapshot' AND owner_id LIKE 'calendar-schedule-%' LIMIT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(retained, "play-automatically");
    });
}
