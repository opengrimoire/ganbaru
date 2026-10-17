use crate::events::commit::{CommitRequest, PreparedCommit};
use crate::events::create::{CalendarIntent, CreateRequest};
use crate::reads::native_window::{self, WindowPurpose};
use crate::recurrence::canonical::Window;
use serde_json::{Value, json};

fn request(timing: Value) -> CreateRequest {
    serde_json::from_value(json!({"kind":"create", "draft":{"timing":timing}})).unwrap()
}

fn timed() -> Value {
    json!({"startTime":"2099-05-10T09:00", "endTime":"2099-05-10T10:00", "timezone":"UTC"})
}

fn command(create: &CreateRequest, revision: &str) -> CommitRequest {
    serde_json::from_value(json!({"vaultId":"vault", "vaultGeneration":1,
        "commandId":"create-1", "reviewRevision":revision, "edit":create}))
    .unwrap()
}

#[test]
fn creation_preview_matches_committed_native_family_with_children_and_overlap() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let create: CreateRequest = serde_json::from_value(json!({"kind":"create", "draft":{
            "timing":timed(), "recurrence":{"kind":"set","value":"COUNT=3;FREQ=DAILY"},
            "fields":[{"field":"title","value":""},{"field":"notifications","value":"[0,10]"},
                {"field":"description","value":"<p>Created</p><script>unsafe()</script>"}],
            "attendees":[{"id":"draft-attendee","name":"Guest","email":"guest@example.com",
                "role":"req-participant","status":"accepted","rsvp":true}]
        }}))
        .unwrap();
        let window = Window::new(
            "2099-05-10",
            "2099-05-12",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let now = 4_081_968_000_000;
        let prepared = create.prepare("create-1", now).unwrap();
        let preview = serde_json::to_value(prepared.project("create-1", &window).unwrap()).unwrap();
        assert_eq!(preview["previewedIds"].as_array().unwrap().len(), 3);
        let following_window = Window::new(
            "2099-05-11",
            "2099-05-12",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let following = serde_json::to_value(
            create
                .prepare("create-1", now)
                .unwrap()
                .project("create-1", &following_window)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(following["previewedIds"].as_array().unwrap().len(), 2);
        assert!(
            following["editingId"].is_null(),
            "a later visible occurrence cannot replace the authored anchor"
        );
        let id = preview["editedId"].as_str().unwrap();
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, 2, "preview never persists its rows");
        let commit = command(&create, preview["reviewRevision"].as_str().unwrap());
        let mut tx = pool.begin().await.unwrap();
        let receipt = PreparedCommit::from_creation(
            create.prepare("create-1", now).unwrap().rows,
            "create-1",
        )
        .write(&mut tx)
        .await
        .unwrap();
        commit.record_receipt(&mut tx, &receipt, now).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let mut actual = native_window::read(&mut tx, &window, WindowPurpose::Render, false)
            .await
            .unwrap()
            .expand(&window)
            .unwrap();
        tx.commit().await.unwrap();
        assert!(
            actual
                .occurrences
                .iter()
                .any(|row| row.template_id == "source"),
            "coincident source survives"
        );
        actual.events.retain(|row| row.id == id);
        actual.overrides.retain(|row| row.parent_event_id == id);
        actual.attendees.retain(|row| row.event_id == id);
        actual.occurrences.retain(|row| row.template_id == id);
        assert_eq!(serde_json::to_value(actual).unwrap(), preview["window"]);
        let description: String =
            sqlx::query_scalar("SELECT description FROM calendar_events WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!description.contains("script"));
        assert!(description.contains("Created"));
    });
}

#[test]
fn creation_review_is_clock_stable_and_retry_survives_source_removal_and_restart() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let create = request(timed());
        let first = create.prepare("create-1", 1_000).unwrap();
        let later = create.prepare("create-1", 100_000).unwrap();
        assert_eq!(first.review_revision, later.review_revision);
        let commit = command(&create, &first.review_revision);
        let mut tx = pool.begin().await.unwrap();
        let receipt = PreparedCommit::from_creation(later.rows, "create-1")
            .write(&mut tx)
            .await
            .unwrap();
        commit
            .record_receipt(&mut tx, &receipt, 100_000)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM calendar_events WHERE id = ?")
            .bind(&receipt.edited_id)
            .execute(&pool)
            .await
            .unwrap();
        let mut retry = serde_json::to_value(&commit).unwrap();
        retry["vaultGeneration"] = json!(42);
        let retry: CommitRequest = serde_json::from_value(retry).unwrap();
        sqlx::query("PRAGMA query_only = ON")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(retry.read_only_retry(&pool).await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(&receipt).unwrap()
        );
        let mut changed = serde_json::to_value(commit).unwrap();
        changed["edit"]["draft"]["timing"]["endTime"] = json!("2099-05-10T11:00");
        let changed: CommitRequest = serde_json::from_value(changed).unwrap();
        assert!(changed.read_only_retry(&pool).await.is_err());
    });
}

#[test]
fn invalid_child_reference_rolls_back_created_source_and_receipt() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let create: CreateRequest = serde_json::from_value(json!({"kind":"create", "draft":{
            "timing":timed(), "fields":[{"field":"projectId","value":"missing-project"}]
        }}))
        .unwrap();
        let prepared = create.prepare("create-1", 1_000).unwrap();
        let id = prepared.rows.edited_id.clone();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            PreparedCommit::from_creation(prepared.rows, "create-1")
                .write(&mut tx)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_edit_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
    });
}

#[test]
fn child_write_failure_rolls_back_partial_creation_and_allows_the_same_retry() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let create: CreateRequest = serde_json::from_value(json!({"kind":"create", "draft":{
            "timing":timed(), "fields":[{"field":"notifications","value":"[0,10]"}]
        }}))
        .unwrap();
        let prepared = create.prepare("create-1", 1_000).unwrap();
        let id = prepared.rows.edited_id.clone();
        let commit = command(&create, &prepared.review_revision);
        sqlx::raw_sql(
            "CREATE TRIGGER fail_created_notification BEFORE INSERT ON calendar_event_notifications
            WHEN NEW.offset_minutes = 10 AND NEW.event_id LIKE 'calendar-create-%'
            BEGIN SELECT RAISE(ABORT, 'injected creation child failure'); END;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error = PreparedCommit::from_creation(prepared.rows, "create-1")
            .write(&mut tx)
            .await
            .err()
            .unwrap();
        assert!(error.contains("injected creation child failure"), "{error}");
        let staged: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events WHERE id = ?")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(staged, 1, "failure occurs after the root write");
        tx.rollback().await.unwrap();
        let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events WHERE id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, 0);
        assert!(commit.read_only_retry(&pool).await.unwrap().is_none());
        sqlx::raw_sql("DROP TRIGGER fail_created_notification")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let receipt = PreparedCommit::from_creation(
            create.prepare("create-1", 2_000).unwrap().rows,
            "create-1",
        )
        .write(&mut tx)
        .await
        .unwrap();
        commit
            .record_receipt(&mut tx, &receipt, 2_000)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(receipt.edited_id, id);
        assert!(commit.read_only_retry(&pool).await.unwrap().is_some());
    });
}

#[test]
fn creation_rejects_unreviewed_intent_derived_fields_and_ambiguous_command_shapes() {
    crate::test_support::block_on(async {
        let create = request(timed());
        assert!(
            create
                .prepare_commit("create-1".into(), "0".repeat(64), 1_000)
                .await
                .is_err()
        );
        let mut value = serde_json::to_value(&create).unwrap();
        value["draft"]["fields"] = json!([{"field":"exceptions","value":"[\"2099-05-10\"]"}]);
        assert!(
            serde_json::from_value::<CreateRequest>(value)
                .unwrap()
                .prepare("create-1", 0)
                .is_err()
        );
        for extra in [
            json!({"createdAt":0}),
            json!({"selection":{"templateId":"source"}}),
            json!({"action":"end_now"}),
        ] {
            let mut value = serde_json::to_value(&create).unwrap();
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(serde_json::from_value::<CalendarIntent>(value).is_err());
        }
    });
}

#[test]
fn creation_preserves_authored_gap_explicit_fold_and_floating_dates() {
    for (timing, start, end) in [
        (
            json!({"startTime":"2026-03-08T02:00", "endTime":"2026-03-08T02:30", "timezone":"America/New_York"}),
            "2026-03-08T02:00:00",
            "2026-03-08T02:30:00",
        ),
        (
            json!({"startTime":"2026-11-01T01:15:00-05:00", "endTime":"2026-11-01T01:45:00-05:00", "timezone":"America/New_York"}),
            "2026-11-01T06:15:00.000Z",
            "2026-11-01T06:45:00.000Z",
        ),
        (
            json!({"startTime":"2026-03-08", "endTime":"2026-03-09", "timezone":"Pacific/Kiritimati", "allDay":true}),
            "2026-03-08",
            "2026-03-09",
        ),
        (
            json!({"startTime":"2026-03-08", "endTime":"2026-03-08", "timezone":"Pacific/Kiritimati", "allDay":true}),
            "2026-03-08",
            "2026-03-08",
        ),
        (
            json!({"startTime":"2026-03-08T09:00", "endTime":"2026-03-08T10:00", "timezone":"America/New_York", "inputZone":"Asia/Tokyo"}),
            "2026-03-08T00:00:00.000Z",
            "2026-03-08T01:00:00.000Z",
        ),
    ] {
        let create = request(timing);
        let prepared = create.prepare("create-1", 0).unwrap();
        let window = Window::new(
            "2026-01-01",
            "2026-12-31",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let preview = serde_json::to_value(prepared.project("create-1", &window).unwrap()).unwrap();
        assert_eq!(preview["window"]["events"][0]["start_time"], start);
        assert_eq!(preview["window"]["events"][0]["end_time"], end);
    }
    for timing in [
        json!({"startTime":"2026-03-09", "endTime":"2026-03-08", "timezone":"UTC", "allDay":true}),
        json!({"startTime":"2026-03-08T09:00", "timezone":"UTC"}),
        json!({"startTime":"2026-03-08T09:00", "endTime":"2026-03-08T10:00", "timezone":"Invalid/Zone"}),
    ] {
        assert!(request(timing).prepare("create-1", 0).is_err());
    }
}
