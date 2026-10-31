//! Restoring Calendar rows, and tasks linked to events, through recovery offers between a hub
//! and a phone.

use super::block_on;
use super::projects_recovery::Pair;
use ganbaru_sync::manifest::vault::calendar::{CALENDARS, EVENTS};
use ganbaru_sync::manifest::vault::projects::TASKS;

/// Publishes the built-in Local calendar from the hub. A real vault seeds it as a capture when
/// Calendar starts to replicate, but these tests never seed, so each replica would keep its own
/// copy that the engine does not know.
pub(super) async fn publish_local_calendar(pair: &mut Pair) {
    for replica in [&pair.hub, &pair.phone] {
        sqlx::raw_sql(
            "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
             DELETE FROM calendars WHERE id = 'local';
             UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
        )
        .execute(&replica.pool)
        .await
        .expect("remove unpublished Local calendar");
    }
    pair.hub_edit(
        "INSERT INTO calendars (id, name, color, source, visible, read_only)
         VALUES ('local', 'Ganbaru', '', 'local', 1, 0)",
    )
    .await;
    pair.converge().await;
}

/// Event `event_id` in `calendar_id` with alarm `alarm-1`.
pub(super) fn event_sql(event_id: &str, calendar_id: &str) -> String {
    format!(
        "INSERT INTO calendar_events (id, title, start_time, end_time, calendar_id)
         VALUES ('{event_id}', 'Review', '2026-10-24T09:00:00Z', '2026-10-24T10:00:00Z',
                 '{calendar_id}');
         INSERT INTO calendar_event_alarms (id, event_id, trigger_value)
         VALUES ('alarm-{event_id}', '{event_id}', '-PT15M');"
    )
}

fn link_sql(task_id: &str, event_id: &str) -> String {
    let key = ganbaru_projects::keys::task_event_key(task_id, event_id).expect("link key");
    format!(
        "INSERT INTO project_task_event_links (task_id, event_id, link_kind, sync_key)
         VALUES ('{task_id}', '{event_id}', 'scheduled', '{key}')"
    )
}

const RETITLE_EVENT_ON_PHONE: &str =
    "UPDATE calendar_events SET title = 'Review again' WHERE id = 'event-1'";

#[test]
fn a_task_deleted_while_another_device_links_it_to_an_event_is_restored_with_the_link() {
    block_on(async {
        let mut pair = Pair::new("calendar-recovery-task-link").await;
        publish_local_calendar(&mut pair).await;
        pair.hub_edit(&event_sql("event-1", "local")).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM project_tasks WHERE id = 'task-1'")
            .await;
        pair.phone_edit(&link_sql("task-1", "event-1")).await;
        pair.converge().await;
        assert!(
            pair.texts("SELECT task_id FROM project_task_event_links".to_string())
                .await
                .is_empty()
        );
        assert_eq!(pair.offers().await, ["task-1"]);

        let task = pair.restore(&TASKS, "task-1").await;
        let key = ganbaru_projects::keys::task_event_key(&task, "event-1").expect("link key");
        assert_eq!(
            pair.texts(
                "SELECT task_id || '|' || event_id || '|' || link_kind || '|' || sync_key
                 FROM project_task_event_links"
                    .to_string()
            )
            .await,
            [format!("{task}|event-1|scheduled|{key}")]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title FROM project_tasks WHERE id = '{task}'"
            ))
            .await,
            ["Write"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_restored_event_comes_back_with_new_ids_for_its_alarms() {
    block_on(async {
        let mut pair = Pair::new("calendar-recovery-event").await;
        publish_local_calendar(&mut pair).await;
        pair.hub_edit(&event_sql("event-1", "local")).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM calendar_events WHERE id = 'event-1'")
            .await;
        pair.phone_edit(RETITLE_EVENT_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["event-1"]);

        let event = pair.restore(&EVENTS, "event-1").await;
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || calendar_id FROM calendar_events WHERE id = '{event}'"
            ))
            .await,
            ["Review again|local"]
        );
        let alarms = pair
            .texts(format!(
                "SELECT id || '|' || trigger_value FROM calendar_event_alarms
                 WHERE event_id = '{event}'"
            ))
            .await;
        assert_eq!(alarms.len(), 1, "{alarms:?}");
        assert!(alarms[0].ends_with("|-PT15M"), "{alarms:?}");
        assert!(!alarms[0].starts_with("alarm-event-1|"), "{alarms:?}");
        pair.finish().await;
    });
}

#[test]
fn a_removed_calendar_restores_its_edited_events_into_local() {
    block_on(async {
        let mut pair = Pair::new("calendar-recovery-calendar").await;
        publish_local_calendar(&mut pair).await;
        pair.hub_edit(&format!(
            "INSERT INTO calendars (id, name) VALUES ('calendar-1', 'Team');
             {}",
            event_sql("event-1", "calendar-1")
        ))
        .await;
        pair.converge().await;
        pair.hub_edit(
            "DELETE FROM calendar_events WHERE calendar_id = 'calendar-1';
             DELETE FROM calendars WHERE id = 'calendar-1';",
        )
        .await;
        pair.phone_edit(RETITLE_EVENT_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["calendar-1"]);

        let event = pair.restore(&CALENDARS, "calendar-1").await;
        assert!(
            pair.texts("SELECT id FROM calendars WHERE id = 'calendar-1'".to_string())
                .await
                .is_empty()
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || calendar_id FROM calendar_events WHERE id = '{event}'"
            ))
            .await,
            ["Review again|local"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT count(*) || '' FROM calendar_event_alarms WHERE event_id = '{event}'"
            ))
            .await,
            ["1"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_removed_calendar_restores_an_event_added_to_it_concurrently_into_local() {
    block_on(async {
        let mut pair = Pair::new("calendar-recovery-calendar-add").await;
        publish_local_calendar(&mut pair).await;
        pair.hub_edit("INSERT INTO calendars (id, name) VALUES ('calendar-1', 'Team')")
            .await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM calendars WHERE id = 'calendar-1'")
            .await;
        pair.phone_edit(&event_sql("event-added", "calendar-1"))
            .await;
        pair.converge().await;
        assert!(
            pair.texts("SELECT id FROM calendar_events".to_string())
                .await
                .is_empty()
        );
        assert_eq!(pair.offers().await, ["calendar-1"]);

        let event = pair.restore(&CALENDARS, "calendar-1").await;
        assert!(
            pair.texts("SELECT id FROM calendars WHERE id = 'calendar-1'".to_string())
                .await
                .is_empty()
        );
        assert_eq!(
            pair.texts("SELECT title || '|' || calendar_id FROM calendar_events".to_string())
                .await,
            ["Review|local"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT trigger_value FROM calendar_event_alarms WHERE event_id = '{event}'"
            ))
            .await,
            ["-PT15M"]
        );
        pair.finish().await;
    });
}
