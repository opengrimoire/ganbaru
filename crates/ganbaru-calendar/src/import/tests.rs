use std::collections::HashMap;

use super::diff::{LinkMode, plan_event, write_event};
use super::preservation::{PreservationOutcome, replace_preservation};
use super::validation::{
    timing_refusal, validate_color, validate_enum, validate_event, validate_json_option,
    validate_payload, validate_preservation, validate_priority, value_refusal,
};
use super::{
    CalendarBulkImportPayload, CalendarImportAlarm, CalendarImportAttendee, CalendarImportEvent,
    CalendarImportOverride, CalendarImportPreservation, CalendarImportPreservedComponent,
    CalendarImportPreservedObject, ExistingEventRow, MAX_ICALENDAR_COMPONENT_DEPTH,
    MAX_IMPORT_EVENT_ATTENDEES, MAX_IMPORT_EVENTS, MAX_PRESERVED_JCAL_BYTES, bulk_import,
    contains_older_revisions,
};

/// Writes `event` as the local calendar's `event-1`, as a new event or over the stored one.
async fn write_one(
    conn: &mut sqlx::SqliteConnection,
    now: &str,
    event: &CalendarImportEvent,
    stored: bool,
    mode: LinkMode,
) {
    let plan = plan_event(conn, "local", "event-1", stored, event, mode, now)
        .await
        .unwrap();
    write_event(conn, "local", now, event, plan).await.unwrap();
}

#[test]
fn validates_calendar_import_enums() {
    assert!(validate_enum("confirmed", "status", &["confirmed"]).is_ok());
    assert!(validate_enum("cancelled", "status", &["confirmed"]).is_err());
}

#[test]
fn rejects_confidential_import_visibility() {
    let mut event = import_event("event-1", "");
    event.visibility = "confidential".to_string();
    assert!(validate_event(&event).is_err());
}

#[test]
fn validates_calendar_import_color_and_priority_ranges() {
    assert!(validate_color(Some(0), "color").is_ok());
    assert!(validate_color(Some(31), "color").is_ok());
    assert!(validate_color(Some(32), "color").is_err());
    assert!(validate_priority(Some(9)).is_ok());
    assert!(validate_priority(Some(10)).is_err());
}

#[test]
fn validates_json_payload_fields() {
    assert!(validate_json_option(&Some(r#"["a"]"#.to_string()), "categories").is_ok());
    assert!(validate_json_option(&Some("not json".to_string()), "categories").is_err());
}

#[test]
fn validates_icalendar_preservation_payload() {
    let preservation = preservation_payload("source.ics");
    assert!(validate_preservation(&preservation).is_ok());

    let invalid_json = CalendarImportPreservation {
        objects: vec![CalendarImportPreservedObject {
            raw_jcal: "not json".to_string(),
            ..preserved_object("object-1")
        }],
        ..preservation_payload("source.ics")
    };
    assert!(validate_preservation(&invalid_json).is_err());

    let invalid_status = CalendarImportPreservation {
        objects: vec![CalendarImportPreservedObject {
            components: vec![CalendarImportPreservedComponent {
                preservation_status: "mystery".to_string(),
                ..preserved_component("component-1", "vtodo")
            }],
            ..preserved_object("object-1")
        }],
        ..preservation_payload("source.ics")
    };
    assert!(validate_preservation(&invalid_status).is_err());
}

fn nested_component(levels: usize) -> CalendarImportPreservedComponent {
    let mut component = preserved_component("component-leaf", "x-nested");
    for level in 1..levels {
        component = CalendarImportPreservedComponent {
            components: vec![component],
            ..preserved_component(&format!("component-{level}"), "x-nested")
        };
    }
    component
}

#[test]
fn preservation_rejects_trees_deeper_than_export_can_rebuild() {
    let at_limit = CalendarImportPreservation {
        objects: vec![CalendarImportPreservedObject {
            components: vec![nested_component(MAX_ICALENDAR_COMPONENT_DEPTH - 1)],
            ..preserved_object("object-1")
        }],
        ..preservation_payload("source.ics")
    };
    assert!(validate_preservation(&at_limit).is_ok());

    let too_deep = CalendarImportPreservation {
        objects: vec![CalendarImportPreservedObject {
            components: vec![nested_component(MAX_ICALENDAR_COMPONENT_DEPTH)],
            ..preserved_object("object-1")
        }],
        ..preservation_payload("source.ics")
    };
    assert!(
        validate_preservation(&too_deep)
            .expect_err("a tree past the depth limit is rejected")
            .contains("nesting limit")
    );
}

#[test]
fn preservation_rejects_jcal_past_the_total_byte_limit() {
    let large_property = "x".repeat(MAX_PRESERVED_JCAL_BYTES / 2);
    let raw_jcal = format!(r#"["x-large",[["x-data",{{}},"text","{large_property}"]],[]]"#);
    let oversized = CalendarImportPreservation {
        objects: vec![CalendarImportPreservedObject {
            components: vec![
                CalendarImportPreservedComponent {
                    raw_jcal: raw_jcal.clone(),
                    ..preserved_component("component-1", "x-large")
                },
                CalendarImportPreservedComponent {
                    raw_jcal,
                    ..preserved_component("component-2", "x-large")
                },
            ],
            ..preserved_object("object-1")
        }],
        ..preservation_payload("source.ics")
    };
    assert!(
        validate_preservation(&oversized)
            .expect_err("preserved jCal past the byte limit is rejected")
            .contains("preserved jCal limit")
    );
}

#[test]
fn stores_icalendar_preserved_objects_and_components() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut tx = pool.begin().await.unwrap();
        let preservation = preservation_payload("source.ics");

        replace_preservation(
            &mut tx,
            "local",
            "2026-05-09 10:00:00",
            &preservation,
            &PreservationOutcome::default(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let object_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
            .fetch_one(&pool)
            .await
            .unwrap();
        let component_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_components")
            .fetch_one(&pool)
            .await
            .unwrap();
        let nested_parent_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM icalendar_components WHERE parent_component_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(object_count, 1);
        assert_eq!(component_count, 3);
        assert_eq!(nested_parent_count, 2);
    });
}

#[test]
fn stores_icalendar_preserved_object_values() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut tx = pool.begin().await.unwrap();
        let mut preservation = preservation_payload("source.ics");
        preservation.objects[0].components[0].raw_jcal =
            r#"["vevent",[["rrule",{},"recur",{"freq":"DAILY","count":3}]],[]]"#.to_string();

        replace_preservation(
            &mut tx,
            "local",
            "2026-05-09 10:00:00",
            &preservation,
            &PreservationOutcome::default(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let object_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM icalendar_value_nodes WHERE value_kind = 'object'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let freq_value: String = sqlx::query_scalar(
            "SELECT text_value
                 FROM icalendar_value_nodes
                 WHERE object_key = 'freq'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(object_count, 1);
        assert_eq!(freq_value, "DAILY");
    });
}

#[test]
fn replacing_icalendar_preservation_removes_previous_source_rows() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut tx = pool.begin().await.unwrap();
        let first = preservation_payload("source.ics");
        let second = CalendarImportPreservation {
            objects: vec![preserved_object("object-2")],
            ..preservation_payload("source.ics")
        };

        replace_preservation(
            &mut tx,
            "local",
            "2026-05-09 10:00:00",
            &first,
            &PreservationOutcome::default(),
        )
        .await
        .unwrap();
        replace_preservation(
            &mut tx,
            "local",
            "2026-05-09 10:05:00",
            &second,
            &PreservationOutcome::default(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM icalendar_objects ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec!["object-2".to_string()]);
    });
}

#[test]
fn every_object_of_one_import_shares_an_import_id_that_differs_between_imports() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut tx = pool.begin().await.unwrap();
        let multiple = CalendarImportPreservation {
            objects: vec![preserved_object("object-1"), preserved_object("object-2")],
            ..preservation_payload("first.ics")
        };
        let other = CalendarImportPreservation {
            objects: vec![preserved_object("object-3")],
            ..preservation_payload("second.ics")
        };
        for payload in [&multiple, &other] {
            replace_preservation(
                &mut tx,
                "local",
                "2026-05-09 10:00:00",
                payload,
                &PreservationOutcome::default(),
            )
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();

        let imports: Vec<String> =
            sqlx::query_scalar("SELECT import_id FROM icalendar_objects ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0], imports[1], "one import command, one import id");
        assert_ne!(imports[0], imports[2], "each import command takes a new id");
        sqlx::query("UPDATE icalendar_objects SET import_id = 'changed'")
            .execute(&pool)
            .await
            .expect_err("an import id never changes");
    });
}

#[test]
fn reimporting_an_unchanged_source_changes_no_row() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let first = bulk_import(&pool, linked_import("first", "2026-05-09 10:00:00"))
            .await
            .unwrap();
        assert_eq!(first.added, 1, "{:?}", first.warnings);
        let before = calendar_rows(&pool).await;

        // The parser mints fresh row and component ids on every read of the same file.
        let again = bulk_import(&pool, linked_import("second", "2026-05-09 11:00:00"))
            .await
            .unwrap();

        assert_eq!(again.updated, 1, "{:?}", again.warnings);
        assert_eq!(calendar_rows(&pool).await, before);
    });
}

#[test]
fn reimporting_a_changed_event_keeps_the_ids_of_its_unchanged_rows() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        bulk_import(&pool, linked_import("first", "2026-05-09 10:00:00"))
            .await
            .unwrap();
        let mut changed = linked_import("second", "2026-05-09 11:00:00");
        changed.events[0].title = "Renamed".to_string();
        changed.events[0].alarms[1].trigger_value = "-PT30M".to_string();

        bulk_import(&pool, changed).await.unwrap();

        let event: (String, String) = sqlx::query_as(
            "SELECT title, updated_at FROM calendar_events WHERE id = 'event-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            event,
            ("Renamed".to_string(), "2026-05-09 11:00:00".to_string())
        );
        let alarms: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, trigger_value FROM calendar_event_alarms ORDER BY sort_order",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            alarms,
            [
                ("alarm-1-first".to_string(), "-PT15M".to_string()),
                ("alarm-2-second".to_string(), "-PT30M".to_string()),
            ]
        );
        let kept: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM calendar_event_attendees
                 UNION ALL SELECT id FROM calendar_event_overrides",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(kept, ["attendee-first", "override-first"]);
        let projected: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, projected_id FROM icalendar_components
                 WHERE projected_kind IN ('alarm', 'override') ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            projected,
            [
                (
                    "component-alarm-second".to_string(),
                    "alarm-1-first".to_string()
                ),
                (
                    "component-override-second".to_string(),
                    "override-first".to_string()
                ),
            ]
        );
    });
}

/// An import of one linked recurring event with every owned row kind, whose payload and
/// component ids end in `suffix` as a fresh parse of the same file would mint them.
fn linked_import(suffix: &str, now: &str) -> CalendarBulkImportPayload {
    let id = |name: &str| format!("{name}-{suffix}");
    let preservation = CalendarImportPreservation {
        objects: vec![CalendarImportPreservedObject {
            components: vec![
                CalendarImportPreservedComponent {
                    components: vec![preserved_component(&id("component-alarm"), "valarm")],
                    ..preserved_component(&id("component-event"), "vevent")
                },
                CalendarImportPreservedComponent {
                    recurrence_id: Some("20260510T100000Z".to_string()),
                    ..preserved_component(&id("component-override"), "vevent")
                },
            ],
            ..preserved_object(&id("object"))
        }],
        ..linked_preservation_payload()
    };
    let alarm = |name: &str, trigger: &str, component: Option<String>| CalendarImportAlarm {
        id: id(name),
        icalendar_component_id: component,
        action: "display".to_string(),
        trigger_type: "relative".to_string(),
        trigger_value: trigger.to_string(),
        description: Some("Reminder".to_string()),
    };
    let mut event = import_event(&id("event"), "Parent");
    event.source_uid = Some("linked@example.com".to_string());
    event.rrule = Some("FREQ=DAILY;COUNT=5".to_string());
    event.icalendar_component_id = Some(id("component-event"));
    event.notifications = Some("[15, 5]".to_string());
    event.exceptions = Some(r#"["2026-05-12"]"#.to_string());
    event.categories = Some(r#"["Work", "Deep"]"#.to_string());
    event.extended_properties = Some(r#"{"X-A":"a"}"#.to_string());
    event.organizer = Some(r#"{"name":"Org","email":"org@example.com"}"#.to_string());
    event.attendees = vec![CalendarImportAttendee {
        id: id("attendee"),
        icalendar_component_id: Some(id("component-event")),
        icalendar_property_index: Some(3),
        name: Some("User".to_string()),
        email: "user@example.com".to_string(),
        role: "req-participant".to_string(),
        status: "accepted".to_string(),
        rsvp: true,
    }];
    event.alarms = vec![
        alarm("alarm-1", "-PT15M", Some(id("component-alarm"))),
        alarm("alarm-2", "-PT5M", None),
    ];
    event.overrides = vec![CalendarImportOverride {
        id: id("override"),
        icalendar_component_id: Some(id("component-override")),
        recurrence_id: "2026-05-10T10:00:00Z".to_string(),
        recurrence_range: None,
        title: Some("Moved".to_string()),
        start_time: None,
        end_time: None,
        description: None,
        location: None,
        url: None,
        color: None,
        status: None,
        transparency: None,
        visibility: None,
        extended_properties: Some(r#"{"X-B":"b"}"#.to_string()),
    }];
    let mut payload = bulk_payload(vec![event]);
    payload.now = now.to_string();
    payload.preservation = Some(preservation);
    payload
}

/// Every row of the Calendar event and iCalendar preservation tables, quoted, in table order.
async fn calendar_rows(pool: &sqlx::SqlitePool) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master
             WHERE type = 'table' AND (name LIKE 'calendar_event%' OR name LIKE 'icalendar_%')
             ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut rows = Vec::new();
    for table in tables {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(pool)
                .await
                .unwrap();
        let values: Vec<String> = columns
            .iter()
            .map(|column| format!("quote({column})"))
            .collect();
        let sql = format!(
            "SELECT '{table}: ' || {} FROM {table} ORDER BY rowid",
            values.join(" || ', ' || ")
        );
        rows.extend(
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
                .fetch_all(pool)
                .await
                .unwrap(),
        );
    }
    rows
}

#[test]
fn linked_import_rows_reference_preserved_components() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let preservation = linked_preservation_payload();
        let mut event = import_event("event-1", "Parent");
        event.icalendar_component_id = Some("component-event".to_string());
        event.attendees = vec![CalendarImportAttendee {
            id: "attendee-1".to_string(),
            icalendar_component_id: Some("component-event".to_string()),
            icalendar_property_index: Some(3),
            name: Some("User".to_string()),
            email: "user@example.com".to_string(),
            role: "req-participant".to_string(),
            status: "accepted".to_string(),
            rsvp: true,
        }];
        event.alarms = vec![CalendarImportAlarm {
            id: "alarm-1".to_string(),
            icalendar_component_id: Some("component-alarm".to_string()),
            action: "display".to_string(),
            trigger_type: "relative".to_string(),
            trigger_value: "-PT15M".to_string(),
            description: Some("Reminder".to_string()),
        }];
        event.overrides = vec![CalendarImportOverride {
            id: "override-1".to_string(),
            icalendar_component_id: Some("component-override".to_string()),
            recurrence_id: "2026-05-10T10:00:00Z".to_string(),
            recurrence_range: None,
            title: Some("Moved".to_string()),
            start_time: None,
            end_time: None,
            description: None,
            location: None,
            url: None,
            color: None,
            status: None,
            transparency: None,
            visibility: None,
            extended_properties: None,
        }];

        let mut payload = bulk_payload(vec![event]);
        payload.preservation = Some(preservation);
        let summary = bulk_import(&pool, payload).await.unwrap();
        assert_eq!(summary.added, 1, "{:?}", summary.warnings);

        let event_component: String = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_events WHERE id = 'event-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let attendee_link: (String, i64) = sqlx::query_as(
            "SELECT icalendar_component_id, icalendar_property_index
                 FROM calendar_event_attendees WHERE id = 'attendee-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let alarm_component: String = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_event_alarms WHERE id = 'alarm-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let override_component: String = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_event_overrides WHERE id = 'override-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let projected_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT projected_kind, projected_id
                 FROM icalendar_components
                 WHERE id IN ('component-event', 'component-alarm', 'component-override')
                 ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(event_component, "component-event");
        assert_eq!(attendee_link, ("component-event".to_string(), 3));
        assert_eq!(alarm_component, "component-alarm");
        assert_eq!(override_component, "component-override");
        assert_eq!(
            projected_rows,
            vec![
                ("alarm".to_string(), "alarm-1".to_string()),
                ("event".to_string(), "event-1".to_string()),
                ("override".to_string(), "override-1".to_string()),
            ]
        );
    });
}

#[test]
fn updating_without_preservation_replacement_keeps_existing_event_link() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut tx = pool.begin().await.unwrap();
        let preservation = linked_preservation_payload();
        let mut initial = import_event("event-1", "Initial");
        initial.icalendar_component_id = Some("component-event".to_string());
        let mut older = import_event("event-1", "Older");
        older.icalendar_component_id = Some("missing-new-component".to_string());

        replace_preservation(
            &mut tx,
            "local",
            "2026-05-09 10:00:00",
            &preservation,
            &PreservationOutcome::default(),
        )
        .await
        .unwrap();
        write_one(
            &mut tx,
            "2026-05-09 10:00:00",
            &initial,
            false,
            LinkMode::Relink,
        )
        .await;
        write_one(&mut tx, "2026-05-09 10:05:00", &older, true, LinkMode::Keep).await;
        tx.commit().await.unwrap();

        let event_component: String = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_events WHERE id = 'event-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(event_component, "component-event");
    });
}

#[test]
fn detects_older_revisions_before_replacing_preservation() {
    let mut existing = HashMap::new();
    existing.insert(
        "event@example.com".to_string(),
        ExistingEventRow {
            id: "event-1".to_string(),
            sequence: 2,
        },
    );
    let mut event = import_event("event-1", "Older");
    event.source_uid = Some("event@example.com".to_string());
    event.sequence = 1;

    assert!(contains_older_revisions(&existing, &[event]));
}

#[test]
fn imported_event_descriptions_are_sanitized_before_persistence() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let event = import_event(
            "event-1",
            "<p onclick=\"alert(1)\">Safe <strong>text</strong></p>",
        );
        let mut tx = pool.begin().await.unwrap();

        write_one(
            &mut tx,
            "2026-05-09 10:00:00",
            &event,
            false,
            LinkMode::Keep,
        )
        .await;
        tx.commit().await.unwrap();

        let description: String =
            sqlx::query_scalar("SELECT description FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(description, "<p>Safe <strong>text</strong></p>");
    });
}

#[test]
fn updated_imported_event_descriptions_are_sanitized_before_persistence() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let initial = import_event("event-1", "Initial");
        let updated = import_event(
            "event-1",
            "<div><img src=\"x\" onerror=\"alert(1)\"><u>Override</u></div>",
        );
        let mut tx = pool.begin().await.unwrap();

        write_one(
            &mut tx,
            "2026-05-09 10:00:00",
            &initial,
            false,
            LinkMode::Keep,
        )
        .await;
        write_one(
            &mut tx,
            "2026-05-09 10:05:00",
            &updated,
            true,
            LinkMode::Keep,
        )
        .await;
        tx.commit().await.unwrap();

        let description: String =
            sqlx::query_scalar("SELECT description FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(description, "<div><u>Override</u></div>");
    });
}

#[test]
fn imported_override_descriptions_are_sanitized_before_persistence() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut event = import_event("event-1", "Parent");
        event.overrides = vec![CalendarImportOverride {
            id: "override-1".to_string(),
            icalendar_component_id: None,
            recurrence_id: "2026-05-10".to_string(),
            recurrence_range: None,
            title: None,
            start_time: None,
            end_time: None,
            description: Some("<p><script>alert(1)</script><u>Safe</u></p>".to_string()),
            location: None,
            url: None,
            color: None,
            status: None,
            transparency: None,
            visibility: None,
            extended_properties: None,
        }];
        let mut tx = pool.begin().await.unwrap();

        write_one(
            &mut tx,
            "2026-05-09 10:00:00",
            &event,
            false,
            LinkMode::Keep,
        )
        .await;
        tx.commit().await.unwrap();

        let description: String = sqlx::query_scalar(
            "SELECT description FROM calendar_event_overrides WHERE id = 'override-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(description, "<p><u>Safe</u></p>");
    });
}

#[test]
fn rejects_overrides_that_canonical_expansion_cannot_decode() {
    let override_row = |start: Option<&str>, end: Option<&str>| CalendarImportOverride {
        id: "override-1".to_string(),
        icalendar_component_id: None,
        recurrence_id: "2026-05-10T15:00:00Z".to_string(),
        recurrence_range: None,
        title: None,
        start_time: start.map(str::to_string),
        end_time: end.map(str::to_string),
        description: None,
        location: None,
        url: None,
        color: None,
        status: None,
        transparency: None,
        visibility: None,
        extended_properties: None,
    };
    let mut event = import_event("event-1", "");
    event.rrule = Some("FREQ=DAILY;COUNT=3".to_string());

    event.overrides = vec![override_row(
        Some("2026-05-10T16:00:00Z"),
        Some("2026-05-10T17:00:00Z"),
    )];
    assert!(validate_event(&event).is_ok());
    event.overrides = vec![override_row(Some("2026-05-10T16:00:00Z"), None)];
    assert!(validate_event(&event).is_ok());

    for (start, end) in [
        (Some("2026-05-10T16:00:00Z"), Some("2026-05-10T16:00:00Z")),
        (Some("2026-05-10T17:00:00Z"), Some("2026-05-10T16:00:00Z")),
        (Some("not a time"), None),
    ] {
        event.overrides = vec![override_row(start, end)];
        let error = validate_event(&event).unwrap_err();
        assert!(error.starts_with("override override-1:"), "{error}");
    }
    event.overrides = vec![CalendarImportOverride {
        recurrence_id: "garbage".to_string(),
        ..override_row(None, None)
    }];
    assert!(validate_event(&event).is_err());
}

fn bulk_payload(events: Vec<CalendarImportEvent>) -> CalendarBulkImportPayload {
    CalendarBulkImportPayload {
        target_calendar_id: "local".to_string(),
        now: "2026-05-09 10:00:00".to_string(),
        preservation: None,
        events,
    }
}

#[test]
fn import_skips_one_oversized_or_unreadable_event_and_keeps_the_rest() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let attendee = |index: usize| CalendarImportAttendee {
            id: format!("attendee-{index}"),
            icalendar_component_id: None,
            icalendar_property_index: None,
            name: None,
            email: format!("guest{index}@example.com"),
            role: "req-participant".to_string(),
            status: "needs-action".to_string(),
            rsvp: false,
        };
        let mut crowded = import_event("crowded", "");
        crowded.attendees = (0..=MAX_IMPORT_EVENT_ATTENDEES).map(attendee).collect();
        let mut unreadable_start = import_event("unreadable-start", "");
        unreadable_start.start_time = "next tuesday".to_string();
        let mut unreadable_exception = import_event("unreadable-exception", "");
        unreadable_exception.rrule = Some("FREQ=DAILY;COUNT=3".to_string());
        unreadable_exception.exceptions = Some(r#"["2026-13-40"]"#.to_string());
        let mut preservation_only = import_event("hourly", "");
        preservation_only.rrule = Some("FREQ=HOURLY;COUNT=3".to_string());
        let mut instant = import_event("instant", "");
        instant.end_time = instant.start_time.clone();

        let summary = bulk_import(
            &pool,
            bulk_payload(vec![
                import_event("kept", ""),
                crowded,
                unreadable_start,
                unreadable_exception,
                preservation_only,
                instant,
            ]),
        )
        .await
        .unwrap();

        assert_eq!(summary.added, 3);
        assert_eq!(summary.warnings.len(), 3, "{:?}", summary.warnings);
        assert!(summary.warnings[0].contains("UID crowded"));
        assert!(summary.warnings[0].contains("over the limit"));
        assert!(summary.warnings[1].contains("UID unreadable-start"));
        assert!(summary.warnings[2].contains("UID unreadable-exception"));
        let stored: Vec<String> =
            sqlx::query_scalar("SELECT source_uid FROM calendar_events ORDER BY source_uid")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(stored, ["hourly", "instant", "kept"]);
    });
}

fn recurring_event_with_overrides(
    id: &str,
    count: usize,
    title_bytes: usize,
) -> CalendarImportEvent {
    let first = chrono::NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();
    let mut event = import_event(id, "");
    event.rrule = Some("FREQ=DAILY".to_string());
    event.overrides = (0..count)
        .map(|index| {
            let day = first + chrono::Days::new(index as u64);
            CalendarImportOverride {
                id: format!("{id}-override-{index}"),
                icalendar_component_id: None,
                recurrence_id: format!("{day}T10:00:00Z"),
                recurrence_range: None,
                title: Some("t".repeat(title_bytes)),
                start_time: None,
                end_time: None,
                description: None,
                location: None,
                url: None,
                color: None,
                status: None,
                transparency: None,
                visibility: None,
                extended_properties: None,
            }
        })
        .collect();
    event
}

/// Exported GEO payloads of the local calendar, by source UID.
async fn exported_geo(pool: &sqlx::SqlitePool) -> Vec<(String, Option<String>)> {
    let snapshot = crate::reads::load_export_snapshot(pool, "local".to_string())
        .await
        .unwrap();
    let snapshot = serde_json::to_value(snapshot).unwrap();
    let mut geo = snapshot["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rows| {
            let event = &rows["event"];
            (
                event["source_uid"].as_str().unwrap().to_string(),
                event["geo"].as_str().map(str::to_string),
            )
        })
        .collect::<Vec<_>>();
    geo.sort();
    geo
}

#[test]
fn geo_round_trips_through_import_and_export_at_stored_precision() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let with_geo = |id: &str, geo: Option<&str>| CalendarImportEvent {
            geo: geo.map(str::to_string),
            ..import_event(id, "")
        };
        let first = vec![
            with_geo("fine", Some(r#"{"lat":37.3860134,"lng":-122.0829316}"#)),
            with_geo("edge", Some(r#"{"lat":-90,"lng":180}"#)),
            with_geo("none", None),
        ];
        bulk_import(&pool, bulk_payload(first)).await.unwrap();

        let exported = exported_geo(&pool).await;
        let payload = |uid: &str| {
            exported
                .iter()
                .find(|(source_uid, _)| source_uid == uid)
                .and_then(|(_, geo)| geo.clone())
        };
        let fine: serde_json::Value = serde_json::from_str(&payload("fine").unwrap()).unwrap();
        assert_eq!(
            fine,
            serde_json::json!({"lat": 37.386013, "lng": -122.082932})
        );
        let edge: serde_json::Value = serde_json::from_str(&payload("edge").unwrap()).unwrap();
        assert_eq!(edge, serde_json::json!({"lat": -90.0, "lng": 180.0}));
        assert_eq!(payload("none"), None);

        // Importing the exported payloads again changes no stored coordinate.
        let stored = || async {
            sqlx::query_as::<_, (String, Option<i64>, Option<i64>)>(
                "SELECT source_uid, geo_lat, geo_lng FROM calendar_events ORDER BY source_uid",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        let before = stored().await;
        assert!(before.contains(&("fine".to_string(), Some(37_386_013), Some(-122_082_932))));
        let again = exported
            .iter()
            .map(|(uid, geo)| with_geo(uid, geo.as_deref()))
            .collect();
        bulk_import(&pool, bulk_payload(again)).await.unwrap();
        assert_eq!(stored().await, before);
        assert_eq!(exported_geo(&pool).await, exported);
    });
}

#[test]
fn import_keeps_owned_row_keys_unique_without_table_constraints() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut repeated = recurring_event_with_overrides("repeated", 2, 1);
        repeated.overrides[1].recurrence_id = repeated.overrides[0].recurrence_id.clone();
        let mut shared_id = recurring_event_with_overrides("shared-id", 2, 1);
        shared_id.overrides[1].id = shared_id.overrides[0].id.clone();
        let mut properties = recurring_event_with_overrides("properties", 1, 1);
        properties.extended_properties = Some(r#"{"X-A":"first","X-A":"second"}"#.to_string());
        properties.overrides[0].extended_properties =
            Some(r#"{"X-B":"first","X-B":"second"}"#.to_string());
        properties.exceptions = Some(r#"["2027-01-03","2027-01-03"]"#.to_string());

        let summary = bulk_import(&pool, bulk_payload(vec![repeated, shared_id, properties]))
            .await
            .unwrap();

        assert_eq!(summary.added, 1, "{:?}", summary.warnings);
        assert_eq!(summary.warnings.len(), 2, "{:?}", summary.warnings);
        assert!(summary.warnings[0].contains("UID repeated"));
        assert!(summary.warnings[0].contains("override recurrence id"));
        assert!(summary.warnings[1].contains("UID shared-id"));
        assert!(summary.warnings[1].contains("override id"));
        let duplicates: i64 = sqlx::query_scalar(
            "SELECT
                    (SELECT COUNT(*) FROM (SELECT 1 FROM calendar_event_overrides
                        GROUP BY parent_event_id, recurrence_id HAVING COUNT(*) > 1))
                  + (SELECT COUNT(*) FROM (SELECT 1 FROM calendar_event_extended_properties
                        GROUP BY event_id, property_key HAVING COUNT(*) > 1))
                  + (SELECT COUNT(*) FROM (SELECT 1 FROM calendar_event_override_extended_properties
                        GROUP BY override_id, property_key HAVING COUNT(*) > 1))
                  + (SELECT COUNT(*) FROM (SELECT 1 FROM calendar_event_exdates
                        GROUP BY event_id, occurrence_date HAVING COUNT(*) > 1))",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(duplicates, 0);
        let values: Vec<String> = sqlx::query_scalar(
            "SELECT property_value FROM calendar_event_extended_properties
                 UNION ALL SELECT property_value FROM calendar_event_override_extended_properties",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(values, ["second", "second"]);
    });
}

#[test]
fn import_refuses_an_event_over_the_timing_bound_and_keeps_its_source() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut normal = import_event("normal", "");
        normal.icalendar_component_id = Some("component-event".to_string());
        let mut oversized = recurring_event_with_overrides("big", 300, 1_000);
        oversized.icalendar_component_id = Some("component-override".to_string());
        let mut payload = bulk_payload(vec![normal, oversized]);
        payload.preservation = Some(linked_preservation_payload());

        let summary = bulk_import(&pool, payload).await.unwrap();

        assert_eq!(summary.added, 1);
        assert_eq!(summary.warnings.len(), 1, "{:?}", summary.warnings);
        assert!(
            summary.warnings[0].starts_with(crate::value_bound::TIMING_TOO_LARGE_CODE),
            "{:?}",
            summary.warnings
        );
        assert!(summary.warnings[0].contains("UID big"));
        let stored: Vec<String> =
            sqlx::query_scalar("SELECT source_uid FROM calendar_events ORDER BY source_uid")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(stored, ["normal"]);
        let projections: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, projected_kind FROM icalendar_components
                 WHERE id IN ('component-event', 'component-override') ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            projections,
            vec![
                ("component-event".to_string(), Some("event".to_string())),
                ("component-override".to_string(), None),
            ]
        );
        let diagnostics: Vec<String> = sqlx::query_scalar(
            "SELECT message FROM icalendar_object_diagnostics WHERE object_id = 'object-1'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("UID big"));
    });
}

#[test]
fn import_leaves_an_event_with_oversized_categories_unprojected_and_imports_the_rest() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut oversized = import_event("big", "");
        let categories = vec!["c".repeat(1_000); 300];
        oversized.categories = Some(serde_json::to_string(&categories).unwrap());
        let summary = bulk_import(
            &pool,
            bulk_payload(vec![import_event("normal", ""), oversized]),
        )
        .await
        .unwrap();

        assert_eq!(summary.added, 1);
        assert_eq!(summary.warnings.len(), 1, "{:?}", summary.warnings);
        assert!(
            summary.warnings[0].starts_with(crate::value_bound::VALUE_TOO_LARGE_CODE),
            "{:?}",
            summary.warnings
        );
        assert!(summary.warnings[0].contains("UID big"));
        assert!(summary.warnings[0].contains("categories"));
        let stored: Vec<String> =
            sqlx::query_scalar("SELECT source_uid FROM calendar_events ORDER BY source_uid")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(stored, ["normal"]);
    });
}

#[test]
fn imported_rows_are_charged_at_least_as_much_as_stored_rows() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut event = import_event("charged", "");
        let fits = |categories: usize| {
            let mut event = import_event("charged", "");
            event.categories =
                Some(serde_json::to_string(&vec!["c".repeat(500); categories]).unwrap());
            value_refusal(&event).is_none()
        };
        let largest = (1..).take_while(|count| fits(*count)).last().unwrap();
        event.categories = Some(serde_json::to_string(&vec!["c".repeat(500); largest]).unwrap());
        bulk_import(&pool, bulk_payload(vec![event])).await.unwrap();
        let id: String =
            sqlx::query_scalar("SELECT id FROM calendar_events WHERE source_uid = 'charged'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut connection = pool.acquire().await.unwrap();
        crate::events::check_stored_value_bounds(&mut connection, &id)
            .await
            .unwrap();
    });
}

#[test]
fn import_timing_bound_accepts_the_largest_event_and_refuses_one_more_override() {
    let fits =
        |count: usize| timing_refusal(&recurring_event_with_overrides("e", count, 400)).is_none();
    let largest = (1..)
        .take_while(|count| fits(*count))
        .last()
        .expect("one override fits");
    assert!(fits(largest));
    assert!(!fits(largest + 1));
    assert!(largest > 100);
}

#[test]
fn import_stores_projection_with_each_component_and_no_ownerless_value_nodes() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut preservation = linked_preservation_payload();
        preservation.objects[0].components[0].raw_jcal = r#"["vevent",[["attendee",{"cn":"Ana","role":["a","b"]},"cal-address","mailto:ana@example.com"],["rrule",{},"recur",{"freq":"DAILY","count":3}]],[]]"#.to_string();
        let mut event = import_event("event-1", "");
        event.icalendar_component_id = Some("component-event".to_string());
        let mut payload = bulk_payload(vec![event]);
        payload.preservation = Some(preservation);

        bulk_import(&pool, payload).await.unwrap();

        let ownerless: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM icalendar_value_nodes WHERE property_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let parameter_nodes: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM icalendar_value_nodes n
                 JOIN icalendar_property_parameters p ON p.id = n.parameter_id
                 WHERE n.property_id = p.property_id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ownerless, 0);
        assert!(parameter_nodes >= 3, "{parameter_nodes}");
    });
}

#[test]
fn import_rejects_more_events_than_the_payload_limit() {
    let events = (0..=MAX_IMPORT_EVENTS)
        .map(|index| import_event(&format!("event-{index}"), ""))
        .collect();
    let error = validate_payload(&bulk_payload(events)).unwrap_err();
    assert!(error.contains("limit of"), "{error}");
    assert!(validate_payload(&bulk_payload(vec![import_event("event-1", "")])).is_ok());
}

async fn in_memory_pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

fn import_event(id: &str, description: &str) -> CalendarImportEvent {
    CalendarImportEvent {
        candidate_id: id.to_string(),
        icalendar_component_id: None,
        title: "Focus".to_string(),
        start_time: "2026-05-09T10:00:00Z".to_string(),
        end_time: "2026-05-09T11:00:00Z".to_string(),
        timezone: "America/Monterrey".to_string(),
        color: None,
        description: description.to_string(),
        rrule: None,
        notifications: None,
        exceptions: None,
        repeat_until: None,
        all_day: false,
        location: String::new(),
        url: String::new(),
        transparency: "opaque".to_string(),
        status: "confirmed".to_string(),
        source_uid: Some(id.to_string()),
        visibility: "public".to_string(),
        priority: None,
        categories: None,
        geo: None,
        sequence: 0,
        rdate: None,
        extended_properties: None,
        organizer: None,
        meeting_enabled: false,
        guest_can_modify: false,
        guest_can_invite_others: true,
        guest_can_see_other_guests: true,
        attendees: Vec::new(),
        alarms: Vec::new(),
        overrides: Vec::new(),
    }
}

fn preservation_payload(source_name: &str) -> CalendarImportPreservation {
    CalendarImportPreservation {
        source_kind: "import-file".to_string(),
        source_name: source_name.to_string(),
        source_fingerprint: "fingerprint".to_string(),
        objects: vec![CalendarImportPreservedObject {
            components: vec![CalendarImportPreservedComponent {
                components: vec![preserved_component("component-2", "valarm")],
                ..preserved_component("component-1", "vtodo")
            }],
            ..preserved_object("object-1")
        }],
    }
}

fn linked_preservation_payload() -> CalendarImportPreservation {
    CalendarImportPreservation {
        source_kind: "import-file".to_string(),
        source_name: "source.ics".to_string(),
        source_fingerprint: "fingerprint".to_string(),
        objects: vec![CalendarImportPreservedObject {
            components: vec![
                CalendarImportPreservedComponent {
                    components: vec![preserved_component("component-alarm", "valarm")],
                    ..preserved_component("component-event", "vevent")
                },
                CalendarImportPreservedComponent {
                    recurrence_id: Some("20260510T100000Z".to_string()),
                    ..preserved_component("component-override", "vevent")
                },
            ],
            ..preserved_object("object-1")
        }],
    }
}

fn preserved_object(id: &str) -> CalendarImportPreservedObject {
    CalendarImportPreservedObject {
        id: id.to_string(),
        prodid: Some("fixture".to_string()),
        version: Some("2.0".to_string()),
        method: Some("PUBLISH".to_string()),
        calendar_scale: Some("GREGORIAN".to_string()),
        raw_jcal: r#"["vcalendar",[],[]]"#.to_string(),
        diagnostics: r#"[]"#.to_string(),
        components: Vec::new(),
    }
}

fn preserved_component(id: &str, component_type: &str) -> CalendarImportPreservedComponent {
    CalendarImportPreservedComponent {
        id: id.to_string(),
        component_type: component_type.to_string(),
        uid: Some(format!("{component_type}@example.com")),
        recurrence_id: None,
        recurrence_id_value_type: None,
        sequence: Some(0),
        dtstart_key: None,
        raw_jcal: format!(r#"["{component_type}",[],[]]"#),
        preservation_status: "unsupported".to_string(),
        projection_warnings: r#"[]"#.to_string(),
        components: Vec::new(),
    }
}
