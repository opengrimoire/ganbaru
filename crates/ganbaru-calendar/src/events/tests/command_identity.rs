//! Every identity a Calendar command derives hashes the command id, so two commands over equal
//! content never produce the same row ids.

use std::collections::BTreeSet;

use crate::events::commit::prepare_rows;
use crate::events::create::CreateRequest;
use crate::events::edit::prepare;
use crate::events::scope::read_snapshot;
use crate::events::task_schedule::ScheduleRequest;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use serde_json::{Value, json};

/// Tables whose rows a command can create or copy, each keyed by an `id` column.
const IDENTIFIED_TABLES: &[&str] = &[
    "calendar_events",
    "calendar_event_archives",
    "calendar_event_overrides",
    "calendar_event_override_extended_properties",
    "calendar_event_attendees",
    "calendar_event_alarms",
    "calendar_event_categories",
    "calendar_event_exdates",
    "calendar_event_rdates",
    "calendar_event_extended_properties",
    "icalendar_objects",
    "icalendar_components",
    "icalendar_component_properties",
    "icalendar_property_parameters",
    "icalendar_value_nodes",
];

async fn seed() -> sqlx::SqlitePool {
    let pool = super::metadata::seed().await;
    sqlx::raw_sql("INSERT INTO project_tasks (id, project_id, section_id, status_id, title, completed_at, due_date, section_order_key, status_order_key)
        VALUES ('task-2', 'project', 'section', 'status', 'Second', '2026-01-01T09:00:00Z', '2099-12-01', 'a2', 'a2');
        UPDATE projects SET default_event_duration_minutes = 120, color = 3;")
        .execute(&pool).await.unwrap();
    pool
}

fn clock(value: &str) -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis(),
        floating_today: None,
    }
}

async fn all_ids(pool: &sqlx::SqlitePool) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for table in IDENTIFIED_TABLES {
        let rows: Vec<String> =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id FROM {table}")))
                .fetch_all(pool)
                .await
                .unwrap();
        ids.extend(rows.into_iter().map(|id| format!("{table}:{id}")));
    }
    ids
}

/// Runs one command twice, each time on a fresh equal vault, and returns the row ids it added.
async fn added_ids<F, Fut>(run: F, command_id: &str) -> BTreeSet<String>
where
    F: Fn(sqlx::SqlitePool, String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let pool = seed().await;
    let before = all_ids(&pool).await;
    run(pool.clone(), command_id.to_string()).await;
    all_ids(&pool).await.difference(&before).cloned().collect()
}

fn assert_distinct(first: &BTreeSet<String>, second: &BTreeSet<String>) {
    assert!(!first.is_empty(), "the command added no rows");
    assert_eq!(
        first.len(),
        second.len(),
        "equal content adds equal row counts"
    );
    let shared: Vec<_> = first.intersection(second).collect();
    assert!(shared.is_empty(), "command ids share row ids: {shared:?}");
}

async fn save_edit(pool: sqlx::SqlitePool, command_id: String, scope: EditScope) {
    let now = clock("2099-05-08T00:00:00Z");
    let draft: Value = json!({"fields":[{"field":"title","value":"Edited"}]});
    let mut tx = pool.begin().await.unwrap();
    let revision = prepare(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        scope,
        now,
        serde_json::from_value(draft.clone()).unwrap(),
    )
    .unwrap()
    .review_revision;
    let rows = prepare_rows(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        scope,
        now,
        serde_json::from_value(draft).unwrap(),
        &command_id,
        &revision,
    )
    .unwrap();
    rows.write(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
}

#[test]
fn split_and_copied_ids_differ_between_command_ids() {
    crate::test_support::block_on(async {
        for scope in [EditScope::Following, EditScope::This] {
            let first = added_ids(move |pool, id| save_edit(pool, id, scope), "save-a").await;
            let second = added_ids(move |pool, id| save_edit(pool, id, scope), "save-b").await;
            // Date rows added to the retained source derive from the source id and the date, so
            // equal dates share a row id by design; every other added row is command scoped.
            let command_scoped = |ids: &BTreeSet<String>| {
                ids.iter()
                    .filter(|id| {
                        !id.starts_with("calendar_event_exdates:")
                            && !id.starts_with("calendar_event_rdates:")
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>()
            };
            assert_distinct(&command_scoped(&first), &command_scoped(&second));
            let again = added_ids(move |pool, id| save_edit(pool, id, scope), "save-a").await;
            assert_eq!(
                first, again,
                "one command id derives the same ids on replay"
            );
        }
    });
}

#[test]
fn created_ids_differ_between_command_ids() {
    crate::test_support::block_on(async {
        let create: CreateRequest = serde_json::from_value(json!({"kind":"create", "draft":{
            "timing":{"startTime":"2099-05-10T09:00", "endTime":"2099-05-10T10:00", "timezone":"UTC"},
            "fields":[{"field":"notifications","value":"[0,10]"}]}}))
        .unwrap();
        let ids = |command_id: &'static str| {
            let create = &create;
            async move {
                let prepared = create.prepare(command_id, 4_081_968_000_000).unwrap();
                prepared.rows.edited_id.clone()
            }
        };
        let first = ids("create-a").await;
        assert_ne!(first, ids("create-b").await);
        assert_eq!(first, ids("create-a").await);
    });
}

#[test]
fn scheduled_ids_differ_between_command_ids() {
    crate::test_support::block_on(async {
        let schedule = |pool: sqlx::SqlitePool, command_id: String| async move {
            let request: ScheduleRequest = serde_json::from_value(json!({
                "kind":"schedule_tasks", "projectId":"project",
                "tasks":[{"id":"task", "revision":0}, {"id":"task-2", "revision":0}],
                "startTime":"2099-05-10T09:00", "timezone":"UTC", "durationMinutes":30,
                "globalIdleTimeoutMinutes":5}))
            .unwrap();
            let mut tx = pool.begin().await.unwrap();
            let prepared = request
                .prepare(request.read(&mut tx).await.unwrap(), &command_id, 1_000)
                .unwrap();
            prepared
                .into_commit(&command_id)
                .write(&mut tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        };
        let first = added_ids(schedule, "schedule-a").await;
        let second = added_ids(schedule, "schedule-b").await;
        assert_distinct(&first, &second);
    });
}

#[test]
fn archive_ids_differ_between_command_ids() {
    crate::test_support::block_on(async {
        let delete = |pool: sqlx::SqlitePool, command_id: String| async move {
            let now = clock("2099-05-08T00:00:00Z");
            let mut tx = pool.begin().await.unwrap();
            let review = crate::events::deletion::prepare(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::All,
                now,
            )
            .unwrap();
            let rows = crate::events::deletion::prepare_commit(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::All,
                now,
                &command_id,
                &review.review_revision,
                false,
            )
            .unwrap();
            rows.write(&mut tx).await.unwrap();
            tx.commit().await.unwrap();
        };
        let first = added_ids(delete, "delete-a").await;
        let second = added_ids(delete, "delete-b").await;
        assert_distinct(&first, &second);
    });
}
