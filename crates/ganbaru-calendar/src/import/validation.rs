use super::{
    CalendarBulkImportPayload, CalendarImportAlarm, CalendarImportAttendee, CalendarImportEvent,
    CalendarImportOverride, CalendarImportPreservation, CalendarImportPreservedComponent,
    CalendarImportPreservedObject, PALETTE_SIZE,
};

pub(super) fn validate_payload(payload: &CalendarBulkImportPayload) -> Result<(), String> {
    require_non_empty(&payload.target_calendar_id, "target_calendar_id")?;
    require_non_empty(&payload.now, "now")?;
    if let Some(preservation) = &payload.preservation {
        validate_preservation(preservation)?;
    }
    for event in &payload.events {
        validate_event(event)?;
    }
    Ok(())
}

pub(super) fn validate_preservation(
    preservation: &CalendarImportPreservation,
) -> Result<(), String> {
    validate_enum(
        &preservation.source_kind,
        "preservation.source_kind",
        &[
            "import-file",
            "import-zip-entry",
            "local-export-base",
            "subscription",
        ],
    )?;
    require_non_empty(
        &preservation.source_fingerprint,
        "preservation.source_fingerprint",
    )?;
    for object in &preservation.objects {
        validate_preserved_object(object)?;
    }
    Ok(())
}

fn validate_preserved_object(object: &CalendarImportPreservedObject) -> Result<(), String> {
    require_non_empty(&object.id, "preservation.object.id")?;
    validate_json_string(&object.raw_jcal, "preservation.object.raw_jcal")?;
    validate_json_string(&object.diagnostics, "preservation.object.diagnostics")?;
    for component in &object.components {
        validate_preserved_component(component)?;
    }
    Ok(())
}

fn validate_preserved_component(
    component: &CalendarImportPreservedComponent,
) -> Result<(), String> {
    require_non_empty(&component.id, "preservation.component.id")?;
    require_non_empty(
        &component.component_type,
        "preservation.component.component_type",
    )?;
    validate_enum(
        &component.preservation_status,
        "preservation.component.preservation_status",
        &[
            "lossless",
            "partial",
            "unsupported",
            "needs-review",
            "regenerated",
            "invalid",
        ],
    )?;
    validate_json_string(&component.raw_jcal, "preservation.component.raw_jcal")?;
    validate_json_string(
        &component.projection_warnings,
        "preservation.component.projection_warnings",
    )?;
    for child in &component.components {
        validate_preserved_component(child)?;
    }
    Ok(())
}

pub(super) fn validate_event(event: &CalendarImportEvent) -> Result<(), String> {
    require_non_empty(&event.candidate_id, "candidate_id")?;
    validate_optional_id(&event.icalendar_component_id, "icalendar_component_id")?;
    require_non_empty(&event.start_time, "start_time")?;
    require_non_empty(&event.end_time, "end_time")?;
    require_non_empty(&event.timezone, "timezone")?;
    validate_color(event.color, "color")?;
    validate_enum(
        &event.transparency,
        "transparency",
        &["opaque", "transparent"],
    )?;
    validate_enum(
        &event.status,
        "status",
        &["confirmed", "tentative", "cancelled"],
    )?;
    validate_enum(&event.visibility, "visibility", &["public", "private"])?;
    validate_priority(event.priority)?;
    validate_non_negative(event.sequence, "sequence")?;
    validate_json_option(&event.notifications, "notifications")?;
    validate_json_option(&event.exceptions, "exceptions")?;
    validate_json_option(&event.categories, "categories")?;
    validate_json_option(&event.geo, "geo")?;
    validate_json_option(&event.rdate, "rdate")?;
    validate_json_option(&event.extended_properties, "extended_properties")?;
    validate_json_option(&event.organizer, "organizer")?;
    for attendee in &event.attendees {
        validate_attendee(attendee)?;
    }
    for alarm in &event.alarms {
        validate_alarm(alarm)?;
    }
    for override_row in &event.overrides {
        validate_override(override_row)?;
    }
    Ok(())
}

fn validate_attendee(attendee: &CalendarImportAttendee) -> Result<(), String> {
    require_non_empty(&attendee.id, "attendee.id")?;
    validate_optional_id(
        &attendee.icalendar_component_id,
        "attendee.icalendar_component_id",
    )?;
    if let Some(index) = attendee.icalendar_property_index {
        validate_non_negative(index, "attendee.icalendar_property_index")?;
    }
    require_non_empty(&attendee.email, "attendee.email")?;
    validate_enum(
        &attendee.role,
        "attendee.role",
        &[
            "chair",
            "req-participant",
            "opt-participant",
            "non-participant",
        ],
    )?;
    validate_enum(
        &attendee.status,
        "attendee.status",
        &[
            "needs-action",
            "accepted",
            "declined",
            "tentative",
            "delegated",
        ],
    )
}

fn validate_alarm(alarm: &CalendarImportAlarm) -> Result<(), String> {
    require_non_empty(&alarm.id, "alarm.id")?;
    validate_optional_id(
        &alarm.icalendar_component_id,
        "alarm.icalendar_component_id",
    )?;
    require_non_empty(&alarm.trigger_value, "alarm.trigger_value")?;
    validate_enum(
        &alarm.action,
        "alarm.action",
        &["display", "audio", "email"],
    )?;
    validate_enum(
        &alarm.trigger_type,
        "alarm.trigger_type",
        &["relative", "absolute"],
    )
}

fn validate_override(override_row: &CalendarImportOverride) -> Result<(), String> {
    require_non_empty(&override_row.id, "override.id")?;
    validate_optional_id(
        &override_row.icalendar_component_id,
        "override.icalendar_component_id",
    )?;
    require_non_empty(&override_row.recurrence_id, "override.recurrence_id")?;
    if let Some(recurrence_range) = &override_row.recurrence_range {
        validate_enum(
            recurrence_range,
            "override.recurrence_range",
            &["this-and-future"],
        )?;
    }
    validate_color(override_row.color, "override.color")?;
    if let Some(status) = &override_row.status {
        validate_enum(
            status,
            "override.status",
            &["confirmed", "tentative", "cancelled"],
        )?;
    }
    if let Some(transparency) = &override_row.transparency {
        validate_enum(
            transparency,
            "override.transparency",
            &["opaque", "transparent"],
        )?;
    }
    if let Some(visibility) = &override_row.visibility {
        validate_enum(visibility, "override.visibility", &["public", "private"])?;
    }
    validate_json_option(
        &override_row.extended_properties,
        "override.extended_properties",
    )
}

pub(super) fn validate_enum(value: &str, field: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("{field} has unsupported value '{value}'"))
    }
}

pub(super) fn validate_color(value: Option<i64>, field: &str) -> Result<(), String> {
    if let Some(color) = value {
        if !(0..PALETTE_SIZE).contains(&color) {
            return Err(format!("{field} is outside the event palette range"));
        }
    }
    Ok(())
}

pub(super) fn validate_priority(value: Option<i64>) -> Result<(), String> {
    if let Some(priority) = value {
        if !(0..=9).contains(&priority) {
            return Err("priority must be between 0 and 9".to_string());
        }
    }
    Ok(())
}

fn validate_non_negative(value: i64, field: &str) -> Result<(), String> {
    if value < 0 {
        Err(format!("{field} cannot be negative"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_json_option(value: &Option<String>, field: &str) -> Result<(), String> {
    if let Some(json) = value {
        validate_json_string(json, field)?;
    }
    Ok(())
}

fn validate_json_string(value: &str, field: &str) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(value)
        .map_err(|e| format!("{field} is not valid JSON: {e}"))?;
    Ok(())
}

fn validate_optional_id(value: &Option<String>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        require_non_empty(value, field)?;
    }
    Ok(())
}

pub(super) fn require_non_empty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} cannot be empty"))
    } else {
        Ok(())
    }
}
