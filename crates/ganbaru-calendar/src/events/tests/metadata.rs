use super::fixtures::*;
use crate::events::metadata::{Metadata, copy_event_metadata};
use crate::events::occurrence::ReadBudget;

#[test]
fn copying_task_links_rejects_a_target_without_the_matching_project() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE calendar_events SET project_id = NULL WHERE id = 'target'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = copy_event_metadata(&mut tx, "source", "target")
            .await
            .unwrap_err();
        assert!(error.contains("outside their project"), "{error}");
        let objects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(objects, 1);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn non_finite_imported_numbers_cannot_collapse_into_the_same_review_hash() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE icalendar_value_nodes SET value_kind = 'number', text_value = NULL, number_value = 1e999 WHERE id = 'value'").execute(&mut *tx).await.unwrap();
        let error = capture(&mut tx).await.revision().unwrap_err();
        assert!(error.contains("non-finite"), "{error}");
        tx.rollback().await.unwrap();
    });
}

pub(super) async fn seed() -> sqlx::SqlitePool {
    let pool = in_memory_pool().await;
    for id in ["source", "target"] {
        insert_test_event_at(
            &pool,
            id,
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            Some("FREQ=DAILY;COUNT=3"),
        )
        .await;
    }
    sqlx::raw_sql(r#"
        INSERT INTO project_groups (id, name) VALUES ('group', 'Group');
        INSERT INTO projects (id, group_id, name) VALUES ('project', 'group', 'Project');
        INSERT INTO project_sections (id, project_id, name) VALUES ('section', 'project', 'General');
        INSERT INTO project_statuses (id, project_id, name, category) VALUES ('status', 'project', 'Open', 'not_started');
        INSERT INTO project_tasks (id, project_id, section_id, status_id, title) VALUES ('task', 'project', 'section', 'status', 'Task');
        UPDATE calendar_events SET project_id = 'project';
        INSERT INTO project_task_event_links (task_id, event_id) VALUES ('task', 'source');
        INSERT INTO icalendar_objects (id, calendar_id, source_kind, source_fingerprint, method, created_at, updated_at)
            VALUES ('object', 'local', 'import-file', 'original', 'request', 'now', 'now');
        INSERT INTO icalendar_components (id, object_id, parent_component_id, calendar_id, component_type, preservation_status, created_at, updated_at)
            VALUES ('root', 'object', NULL, 'local', 'vcalendar', 'lossless', 'now', 'now'),
                   ('event', 'object', 'root', 'local', 'vevent', 'lossless', 'now', 'now'),
                   ('alarm-component', 'object', 'event', 'local', 'valarm', 'lossless', 'now', 'now'),
                   ('zone', 'object', 'root', 'local', 'vtimezone', 'lossless', 'now', 'now'),
                   ('standard', 'object', 'zone', 'local', 'standard', 'lossless', 'now', 'now'),
                   ('other-event', 'object', 'root', 'local', 'vevent', 'lossless', 'now', 'now'),
                   ('override-component', 'object', 'root', 'local', 'vevent', 'lossless', 'now', 'now');
        UPDATE calendar_events SET icalendar_component_id = 'event', source_uid = 'original-uid' WHERE id = 'source';
        INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title, icalendar_component_id)
            VALUES ('override', 'source', '2099-05-10', 'Different', 'override-component');
        INSERT INTO calendar_event_override_extended_properties (id, override_id, property_key, property_value)
            VALUES ('override-property', 'override', 'X-EXTRA', 'override data');
        INSERT INTO calendar_event_attendees (id, event_id, name, email, status, icalendar_component_id, icalendar_property_index)
            VALUES ('attendee', 'source', 'Guest', 'guest@example.test', 'accepted', 'event', 3);
        INSERT INTO calendar_event_alarms (id, event_id, trigger_value, description, icalendar_component_id)
            VALUES ('alarm', 'source', '-PT10M', 'Reminder', 'alarm-component');
        INSERT INTO calendar_event_categories (id, event_id, category, sort_order) VALUES ('category', 'source', 'Work', 7);
        INSERT INTO calendar_event_extended_properties (id, event_id, property_key, property_value) VALUES ('extension', 'source', 'X-EDITABLE', 'value');
        INSERT INTO calendar_event_organizers (event_id, name, email) VALUES ('source', 'Organizer', 'owner@example.test');
        INSERT INTO calendar_event_notifications (id, event_id, offset_minutes) VALUES ('notification', 'source', 12);
        INSERT INTO calendar_event_pomodoro_configs (event_id, rhythm_kind, rhythm_source) VALUES ('source', 'sequence', 'custom');
        INSERT INTO calendar_event_pomodoro_config_sequence_steps (event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes)
            VALUES ('source', 0, 25, 'short_break', 5);
        INSERT INTO music_context_assignments (owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms)
            VALUES ('event-snapshot', 'source', 'focus', 'keep-current-music', 'explicit', 1);
        INSERT INTO icalendar_component_properties (id, component_id, name, value_type)
            VALUES ('property', 'event', 'x-preserved', 'text'), ('zone-property', 'zone', 'tzid', 'text'),
                   ('alarm-property', 'alarm-component', 'x-alarm', 'unknown');
        INSERT INTO icalendar_property_parameters (id, property_id, name) VALUES ('parameter', 'property', 'x-param');
        INSERT INTO icalendar_value_nodes (id, property_id, parameter_id, value_kind, text_value)
            VALUES ('value', 'property', NULL, 'text', 'preserved'), ('parameter-value', NULL, 'parameter', 'text', 'kept'),
                   ('zone-value', 'zone-property', NULL, 'text', 'Custom/Zone'), ('array', 'alarm-property', NULL, 'array', NULL);
        INSERT INTO icalendar_value_nodes (id, property_id, parent_node_id, sort_order, value_kind, text_value)
            VALUES ('child', 'alarm-property', 'array', 0, 'text', 'nested');
        INSERT INTO icalendar_component_projection_warnings (id, component_id, message) VALUES ('warning', 'event', 'Review source');
        INSERT INTO icalendar_object_diagnostics (id, object_id, message) VALUES ('diagnostic', 'object', 'Original import');
    "#).execute(&pool).await.unwrap();
    pool
}

async fn capture(connection: &mut sqlx::SqliteConnection) -> Metadata {
    Metadata::read(connection, "source", &mut ReadBudget::default())
        .await
        .unwrap()
}

#[test]
fn review_revision_covers_children_configurations_and_imported_values_without_parent_touches() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let mut tx = pool.begin().await.unwrap();
        let original = capture(&mut tx).await.revision().unwrap();
        assert_eq!(original, capture(&mut tx).await.revision().unwrap());
        for change in [
            "UPDATE calendar_event_alarms SET trigger_value = '-PT11M'",
            "UPDATE calendar_event_attendees SET icalendar_property_index = 4",
            "UPDATE calendar_event_override_extended_properties SET property_value = 'changed'",
            "UPDATE calendar_event_pomodoro_config_sequence_steps SET focus_duration_minutes = 26",
            "UPDATE music_context_assignments SET behavior = 'pause-music'",
            "UPDATE icalendar_value_nodes SET text_value = 'changed' WHERE id = 'parameter-value'",
            "UPDATE icalendar_components SET preservation_status = 'partial' WHERE id = 'event'",
            "UPDATE calendar_event_notifications SET offset_minutes = 13",
            "UPDATE project_task_event_links SET link_kind = 'reference'",
        ] {
            sqlx::query("SAVEPOINT child_change")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query(change).execute(&mut *tx).await.unwrap();
            assert_ne!(
                original,
                capture(&mut tx).await.revision().unwrap(),
                "{change}"
            );
            sqlx::query("ROLLBACK TO child_change")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("RELEASE child_change")
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        assert_eq!(original, capture(&mut tx).await.revision().unwrap());
        tx.rollback().await.unwrap();
    });
}

#[test]
fn copied_import_has_independent_envelope_alarms_parameters_timezone_and_attendee_provenance() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let mut tx = pool.begin().await.unwrap();
        let before = capture(&mut tx).await.revision().unwrap();
        sqlx::query("INSERT INTO calendars (id, name, source) VALUES ('other', 'Other', 'local')")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE calendar_events SET calendar_id = 'other' WHERE id = 'target'")
            .execute(&mut *tx)
            .await
            .unwrap();
        copy_event_metadata(&mut tx, "source", "target")
            .await
            .unwrap();
        assert_eq!(before, capture(&mut tx).await.revision().unwrap());
        let (component, object, uid, projected): (String,String,String,String) = sqlx::query_as(
            "SELECT c.id, c.object_id, c.uid, c.projected_id FROM calendar_events e JOIN icalendar_components c ON c.id = e.icalendar_component_id WHERE e.id = 'target'")
            .fetch_one(&mut *tx).await.unwrap();
        assert_ne!(component, "event");
        assert_ne!(object, "object");
        assert_eq!(uid, "target");
        assert_eq!(projected, "target");
        let (attendee_component, property_index): (String,i64) = sqlx::query_as("SELECT icalendar_component_id, icalendar_property_index FROM calendar_event_attendees WHERE event_id = 'target'")
            .fetch_one(&mut *tx).await.unwrap();
        assert_eq!(attendee_component, component);
        assert_eq!(property_index, 3);
        let alarm: (String,String) = sqlx::query_as("SELECT a.trigger_value, c.component_type FROM calendar_event_alarms a JOIN icalendar_components c ON c.id = a.icalendar_component_id WHERE a.event_id = 'target'")
            .fetch_one(&mut *tx).await.unwrap();
        assert_eq!(alarm, ("-PT10M".into(), "valarm".into()));
        let components: Vec<String> = sqlx::query_scalar("SELECT component_type FROM icalendar_components WHERE object_id = ? ORDER BY component_type")
            .bind(&object).fetch_all(&mut *tx).await.unwrap();
        assert_eq!(
            components,
            ["standard", "valarm", "vcalendar", "vevent", "vtimezone"]
        );
        let values: Vec<String> = sqlx::query_scalar("SELECT n.text_value FROM icalendar_value_nodes n LEFT JOIN icalendar_property_parameters param ON param.id = n.parameter_id JOIN icalendar_component_properties p ON p.id = COALESCE(n.property_id, param.property_id) JOIN icalendar_components c ON c.id = p.component_id WHERE c.object_id = ? AND n.text_value IS NOT NULL ORDER BY n.text_value")
            .bind(&object).fetch_all(&mut *tx).await.unwrap();
        assert_eq!(values, ["Custom/Zone", "kept", "nested", "preserved"]);
        sqlx::query("DELETE FROM icalendar_objects WHERE id = 'object'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_components WHERE object_id = ?")
                .bind(object)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(remaining, 5);
        let wrong_calendar: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM icalendar_components WHERE calendar_id <> 'other'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(wrong_calendar, 0);
        let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_task_event_links WHERE event_id = 'target' AND task_id = 'task'").fetch_one(&mut *tx).await.unwrap();
        assert_eq!(links, 1);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn metadata_copy_failure_rolls_back_import_graph_and_all_earlier_children() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        sqlx::query("CREATE TRIGGER reject_copy BEFORE INSERT ON calendar_event_categories WHEN NEW.event_id = 'target' BEGIN SELECT RAISE(ABORT, 'injected copy failure'); END")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error = copy_event_metadata(&mut tx, "source", "target")
            .await
            .unwrap_err();
        assert!(error.contains("injected copy failure"), "{error}");
        tx.rollback().await.unwrap();
        let remaining: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM icalendar_objects), (SELECT COUNT(*) FROM calendar_event_alarms WHERE event_id = 'target'), (SELECT COUNT(*) FROM calendar_event_attendees WHERE event_id = 'target')")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(remaining, (1, 0, 0));
        let link: Option<String> = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_events WHERE id = 'target'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(link, None);
    });
}

#[test]
fn malformed_import_graphs_fail_before_copying() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        for change in [
            "UPDATE icalendar_components SET parent_component_id = 'alarm-component' WHERE id = 'event'",
            "UPDATE icalendar_value_nodes SET parent_node_id = 'child' WHERE id = 'array'",
            "UPDATE icalendar_value_nodes SET parent_node_id = 'value' WHERE id = 'child'",
            "UPDATE icalendar_value_nodes SET property_id = NULL WHERE id = 'child'",
        ] {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query(change).execute(&mut *tx).await.unwrap();
            let error = copy_event_metadata(&mut tx, "source", "target")
                .await
                .unwrap_err();
            assert!(
                error.contains("cycle")
                    || error.contains("crosses property owners")
                    || error.contains("uncaptured child owner"),
                "{error}"
            );
            let copies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            assert_eq!(copies, 1);
            tx.rollback().await.unwrap();
        }
    });
}

#[test]
fn preservation_values_share_the_source_allocation_budget() {
    crate::test_support::block_on(async {
        let pool = seed().await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE icalendar_value_nodes SET text_value = printf('%.*c', 8388608, 'x') WHERE id = 'value'").execute(&mut *tx).await.unwrap();
        let error = match Metadata::read(&mut tx, "source", &mut ReadBudget::default()).await {
            Ok(_) => panic!("oversized preservation was accepted"),
            Err(error) => error,
        };
        assert!(error.contains("budget"), "{error}");
        tx.rollback().await.unwrap();
    });
}
