use super::{
    CalendarBulkImportPayload, CalendarImportAlarm, CalendarImportAttendee, CalendarImportEvent,
    CalendarImportOverride, CalendarImportPreservation, CalendarImportPreservedComponent,
    CalendarImportPreservedObject, MAX_ICALENDAR_COMPONENT_DEPTH, MAX_IMPORT_EVENT_ALARMS,
    MAX_IMPORT_EVENT_ATTENDEES, MAX_IMPORT_EVENT_DATES, MAX_IMPORT_EVENT_OVERRIDES,
    MAX_IMPORT_EVENTS, MAX_PRESERVED_JCAL_BYTES, PALETTE_SIZE,
};
use crate::recurrence::canonical::{
    override_date, parse_date, stored_time, validate_override_times,
};
use crate::value_bound::{OverrideColumns, TIMING_TOO_LARGE_CODE, VALUE_TOO_LARGE_CODE, ValueSize};
use ganbaru_civil_time as civil_time;
use jiff::tz::TimeZone;

pub(super) fn validate_payload(payload: &CalendarBulkImportPayload) -> Result<(), String> {
    require_non_empty(&payload.target_calendar_id, "target_calendar_id")?;
    require_non_empty(&payload.now, "now")?;
    if payload.events.len() > MAX_IMPORT_EVENTS {
        return Err(format!(
            "import exceeds the limit of {MAX_IMPORT_EVENTS} events; split the calendar into smaller files"
        ));
    }
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
    let mut jcal_bytes = 0usize;
    for object in &preservation.objects {
        validate_preserved_object(object, &mut jcal_bytes)?;
    }
    Ok(())
}

/// Adds one preserved jCal payload to the import total, rejecting the import past the byte limit.
fn charge_preserved_jcal(total: &mut usize, raw_jcal: &str) -> Result<(), String> {
    *total = total.saturating_add(raw_jcal.len());
    if *total > MAX_PRESERVED_JCAL_BYTES {
        return Err(format!(
            "preservation exceeds the preserved jCal limit of {MAX_PRESERVED_JCAL_BYTES} bytes"
        ));
    }
    Ok(())
}

/// Walks the component tree with an explicit stack so a deep tree is rejected by its depth, not by
/// exhausting the call stack.
fn validate_preserved_object(
    object: &CalendarImportPreservedObject,
    jcal_bytes: &mut usize,
) -> Result<(), String> {
    require_non_empty(&object.id, "preservation.object.id")?;
    charge_preserved_jcal(jcal_bytes, &object.raw_jcal)?;
    validate_json_string(&object.raw_jcal, "preservation.object.raw_jcal")?;
    validate_json_string(&object.diagnostics, "preservation.object.diagnostics")?;
    let root_level = 1;
    let mut pending = object
        .components
        .iter()
        .map(|component| (component, root_level + 1))
        .collect::<Vec<_>>();
    while let Some((component, level)) = pending.pop() {
        if level > MAX_ICALENDAR_COMPONENT_DEPTH {
            return Err(format!(
                "preservation exceeds the component nesting limit of {MAX_ICALENDAR_COMPONENT_DEPTH}"
            ));
        }
        charge_preserved_jcal(jcal_bytes, &component.raw_jcal)?;
        validate_preserved_component(component)?;
        pending.extend(component.components.iter().map(|child| (child, level + 1)));
    }
    Ok(())
}

/// Checks one component's own fields; its children are validated by the caller's walk.
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
    )
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
        validate_override_geometry(event, override_row)?;
    }
    Ok(())
}

/// Explains why one event is left out of an import while the rest still import: it exceeds a
/// per-event limit, or a time it stores cannot be read back by recurrence expansion. Unsupported
/// recurrence rules, unresolvable zones, and empty ranges are kept, so preservation can export
/// them and Calendar windows report them for that event alone.
pub(super) fn skip_reason(event: &CalendarImportEvent) -> Option<String> {
    let exceptions = date_list(&event.exceptions);
    let rdates = date_list(&event.rdate);
    let counts = [
        (
            "attendees",
            event.attendees.len(),
            MAX_IMPORT_EVENT_ATTENDEES,
        ),
        ("alarms", event.alarms.len(), MAX_IMPORT_EVENT_ALARMS),
        (
            "overrides",
            event.overrides.len(),
            MAX_IMPORT_EVENT_OVERRIDES,
        ),
        ("excluded dates", exceptions.len(), MAX_IMPORT_EVENT_DATES),
        ("added dates", rdates.len(), MAX_IMPORT_EVENT_DATES),
    ];
    if let Some((name, count, limit)) = counts.into_iter().find(|(_, count, limit)| count > limit) {
        return Some(format!(
            "{} has {count} {name}, over the limit of {limit}",
            event_label(event)
        ));
    }
    let zone = if event.all_day {
        TimeZone::UTC
    } else {
        civil_time::zone(&event.timezone).unwrap_or(TimeZone::UTC)
    };
    let readable = |value: &str| {
        if value.len() == 10 {
            parse_date(value).map(|_| ())
        } else {
            stored_time(value, &zone, event.all_day).map(|_| ())
        }
    };
    let times = [event.start_time.as_str(), event.end_time.as_str()];
    times
        .into_iter()
        .chain(exceptions.iter().map(String::as_str))
        .chain(rdates.iter().map(String::as_str))
        .find_map(|value| readable(value).err())
        .map(|error| format!("{} has an unreadable time ({error})", event_label(event)))
}

/// Names the event when its timing value is over the sync bound, led by the stable
/// [`TIMING_TOO_LARGE_CODE`] the client localizes. The event is then not imported or projected, and
/// preservation keeps its full source.
pub(super) fn timing_refusal(event: &CalendarImportEvent) -> Option<String> {
    let mut size = ValueSize::default();
    size.add_event(
        &event.start_time,
        &event.end_time,
        &event.timezone,
        event.rrule.as_deref(),
        event.repeat_until.as_deref(),
    );
    for date in date_list(&event.exceptions)
        .iter()
        .chain(date_list(&event.rdate).iter())
    {
        size.add_date_row(None, date);
    }
    for row in &event.overrides {
        size.add_override(&OverrideColumns {
            id: &row.id,
            recurrence_id: &row.recurrence_id,
            recurrence_range: row.recurrence_range.as_deref(),
            title: row.title.as_deref(),
            start_time: row.start_time.as_deref(),
            end_time: row.end_time.as_deref(),
            description: row.description.as_deref(),
            location: row.location.as_deref(),
            url: row.url.as_deref(),
            color: row.color,
            status: row.status.as_deref(),
            transparency: row.transparency.as_deref(),
            visibility: row.visibility.as_deref(),
            icalendar_component_id: row.icalendar_component_id.as_deref(),
        });
        let properties = row
            .extended_properties
            .as_deref()
            .and_then(|json| {
                serde_json::from_str::<std::collections::BTreeMap<String, String>>(json).ok()
            })
            .unwrap_or_default();
        for (key, value) in &properties {
            size.add_override_property(None, &row.id, key, value);
        }
    }
    size.check()
        .is_err()
        .then(|| format!("{TIMING_TOO_LARGE_CODE}: {}", event_label(event)))
}

/// Names the event and its first group of rows whose sync value is over the bound, led by the
/// stable [`VALUE_TOO_LARGE_CODE`]. The event is then not imported or projected, and preservation
/// keeps its full source. Each row is charged at least as much as Calendar charges the stored
/// row, with generated identifiers at their allowance, so an imported event never fails a later
/// edit's check on rows it did not change. Malformed lists are left to the field validation.
pub(super) fn value_refusal(event: &CalendarImportEvent) -> Option<String> {
    fn owned_row(size: &mut ValueSize) {
        size.row();
        size.generated_id();
        size.generated_id();
    }
    let mut alarms = ValueSize::default();
    for alarm in &event.alarms {
        alarms.row();
        alarms.id(&alarm.id);
        alarms.generated_id();
        alarms.optional_id(alarm.icalendar_component_id.as_deref());
        alarms.text(&alarm.action);
        alarms.text(&alarm.trigger_type);
        alarms.text(&alarm.trigger_value);
        alarms.optional_text(alarm.description.as_deref());
        alarms.integer();
    }
    let mut attendees = ValueSize::default();
    for attendee in &event.attendees {
        attendees.row();
        attendees.id(&attendee.id);
        attendees.generated_id();
        attendees.optional_id(attendee.icalendar_component_id.as_deref());
        attendees.optional_integer(attendee.icalendar_property_index);
        attendees.optional_text(attendee.name.as_deref());
        attendees.text(&attendee.email);
        attendees.text(&attendee.role);
        attendees.text(&attendee.status);
        attendees.integer();
        attendees.integer();
    }
    let mut organizer = ValueSize::default();
    if let Ok(Some(row)) = super::parse_organizer(&event.organizer) {
        owned_row(&mut organizer);
        organizer.optional_text(row.name.as_deref());
        organizer.text(&row.email);
    }
    let mut categories = ValueSize::default();
    for category in super::parse_string_list(&event.categories, "categories").unwrap_or_default() {
        owned_row(&mut categories);
        categories.text(&category);
        categories.integer();
    }
    let mut notifications = ValueSize::default();
    for _ in super::parse_i64_list(&event.notifications, "notifications").unwrap_or_default() {
        owned_row(&mut notifications);
        notifications.integer();
        notifications.integer();
    }
    let mut properties = ValueSize::default();
    for (key, value) in super::parse_string_map(&event.extended_properties, "extended_properties")
        .unwrap_or_default()
    {
        owned_row(&mut properties);
        properties.text(&key);
        properties.text(&value);
        properties.integer();
    }
    [
        ("alarms", alarms),
        ("attendees", attendees),
        ("organizer", organizer),
        ("categories", categories),
        ("notifications", notifications),
        ("extended properties", properties),
    ]
    .into_iter()
    .find(|(_, size)| !size.fits())
    .map(|(group, _)| format!("{VALUE_TOO_LARGE_CODE}: {} ({group})", event_label(event)))
}

/// Decodes a validated JSON date list; a non-list value is left to Calendar windows to report.
fn date_list(value: &Option<String>) -> Vec<String> {
    value
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
        .unwrap_or_default()
}

fn event_label(event: &CalendarImportEvent) -> String {
    match event
        .source_uid
        .as_deref()
        .filter(|uid| !uid.trim().is_empty())
    {
        Some(uid) => format!("UID {}", uid.chars().take(120).collect::<String>()),
        None => "an event without UID".to_string(),
    }
}

/// Rejects override identities and times that canonical expansion cannot decode, and timed or
/// all-day ranges that end before they start. An event whose zone does not resolve here is
/// reported for that event alone when Calendar windows decode it.
fn validate_override_geometry(
    event: &CalendarImportEvent,
    override_row: &CalendarImportOverride,
) -> Result<(), String> {
    let home_zone = if event.all_day {
        TimeZone::UTC
    } else {
        match civil_time::zone(&event.timezone) {
            Ok(zone) => zone,
            Err(_) => return Ok(()),
        }
    };
    override_date(&override_row.recurrence_id, &home_zone, event.all_day)
        .and_then(|_| {
            validate_override_times(
                override_row.start_time.as_deref(),
                override_row.end_time.as_deref(),
                &home_zone,
                event.all_day,
            )
        })
        .map_err(|error| format!("override {}: {error}", override_row.id))
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
