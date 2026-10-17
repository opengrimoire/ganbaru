use super::*;
use serde_json::json;

fn draft(value: serde_json::Value) -> EventDraft {
    serde_json::from_value(value).unwrap()
}

#[test]
fn omitted_nullable_fields_remain_unchanged_while_explicit_clear_is_retained() {
    let mut unchanged = draft(json!({}));
    unchanged.validate().unwrap();
    assert!(unchanged.pomodoro_config.is_none());
    assert_eq!(unchanged.recurrence, RecurrenceIntent::Unchanged);
    let mut clear = draft(
        json!({"fields":[{"field":"projectId","value":null}], "pomodoroConfig":{"action":"clear"}, "recurrence":{"kind":"clear"}}),
    );
    clear.validate().unwrap();
    assert!(matches!(
        clear.fields[0],
        CalendarEventUpdateField::ProjectId(None)
    ));
    assert!(matches!(
        clear.pomodoro_config,
        Some(CalendarPomodoroConfigPatch::Clear)
    ));
    assert_eq!(clear.recurrence, RecurrenceIntent::Clear);
}

#[test]
fn requests_cannot_supply_execution_evidence_or_derived_mutation_fields() {
    for key in [
        "now",
        "activeRunId",
        "operations",
        "updatedAt",
        "newId",
        "protectedThrough",
    ] {
        let mut value = json!({"selection":{"templateId":"series","recurrenceDate":"2026-05-15","scope":"all"},"draft":{}});
        value[key] = json!("untrusted");
        assert!(
            serde_json::from_value::<EditRequest>(value).is_err(),
            "{key}"
        );
    }
    for (field, value) in [
        ("exceptions", json!("[]")),
        ("rrule", json!(null)),
        ("repeatUntil", json!(null)),
        ("rdate", json!("[]")),
        ("sourceUid", json!("foreign")),
        ("sequence", json!(1)),
        ("startTime", json!("2026-05-15T09:00:00Z")),
        ("allDay", json!(false)),
    ] {
        assert!(
            draft(json!({"fields":[{"field":field,"value":value}]}))
                .validate()
                .unwrap_err()
                .contains("raw field"),
            "{field}"
        );
    }
}

#[test]
fn duplicate_fields_children_and_music_phases_are_rejected() {
    let mut repeated = draft(
        json!({"fields":[{"field":"projectId","value":"project"},{"field":"projectId","value":null}]}),
    );
    assert!(repeated.validate().unwrap_err().contains("repeats field"));
    let attendee = json!({"id":"a", "email":"a@example.test", "name":null,"role":"chair","status":"accepted","rsvp":false});
    assert!(
        draft(json!({"attendees":[attendee,attendee]}))
            .validate()
            .unwrap_err()
            .contains("attendee identity")
    );
    let alarm = json!({"id":"a","action":"display","triggerType":"relative","triggerValue":"-PT5M","description":null});
    assert!(
        draft(json!({"alarms":[alarm,alarm]}))
            .validate()
            .unwrap_err()
            .contains("alarm identity")
    );
    let assignment = json!({"phase":"focus","behavior":"inherit","playlistId":null,"soundscapeId":null,"soundscapeBehavior":"inherit","provenanceKind":"explicit","provenanceId":null});
    assert!(
        draft(
            json!({"fields":[{"field":"musicOverrideAssignments","value":[assignment,assignment]}]})
        )
        .validate()
        .unwrap_err()
        .contains("phase more than once")
    );
    assert!(
        draft(json!({"fields":[{"field":"musicSnapshotAssignments","value":[assignment]}]}))
            .validate()
            .unwrap_err()
            .contains("provenance")
    );
}

#[test]
fn json_metadata_is_validated_as_its_declared_shape_and_database_range() {
    for (field, value) in [
        ("notifications", json!({"offset":5})),
        ("notifications", json!([-1])),
        ("categories", json!([1])),
        ("extendedProperties", json!({"name":1})),
        ("geo", json!({"lat":91,"lng":0})),
        ("geo", json!({"lat":0,"lng":181})),
        ("organizer", json!({"name":null,"email":" "})),
    ] {
        assert!(
            draft(json!({"fields":[{"field":field,"value":value.to_string()}]}))
                .validate()
                .is_err(),
            "{field}: {value}"
        );
    }
}

#[test]
fn child_and_serialized_byte_limits_reject_oversized_drafts_before_sql() {
    let value = "x".repeat(MAX_DRAFT_BYTES);
    assert!(
        draft(json!({"fields":[{"field":"description","value":value}]}))
            .validate()
            .unwrap_err()
            .contains("byte budget")
    );
    let categories = vec!["category"; MAX_CHILDREN + 1];
    assert!(draft(json!({"fields":[{"field":"categories","value":serde_json::to_string(&categories).unwrap()}]})).validate().unwrap_err().contains("record budget"));
    let alarm = json!({"id":"a","action":"display","triggerType":"relative","triggerValue":"-PT5M","description":null});
    assert!(
        draft(json!({"alarms":vec![alarm; MAX_CHILDREN + 1]}))
            .validate()
            .unwrap_err()
            .contains("record budget")
    );
}

#[test]
fn prepared_description_uses_the_same_sanitizer_as_persistence() {
    let mut value = draft(
        json!({"fields":[{"field":"description","value":"<p>Hello</p><script>alert(1)</script>"}]}),
    );
    value.validate().unwrap();
    let CalendarEventUpdateField::Description(description) = &value.fields[0] else {
        panic!("wrong field");
    };
    assert!(description.contains("Hello"));
    assert!(!description.contains("script"));
}
