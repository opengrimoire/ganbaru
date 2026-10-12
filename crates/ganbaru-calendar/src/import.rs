//! iCalendar bulk import into one calendar, with revision ordering and preservation links.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::description::{
    sanitize_calendar_description_html, sanitize_optional_calendar_description,
};

mod preservation;
mod validation;

use preservation::{link_preserved_component, preservation_link_id, replace_preservation};
use validation::{require_non_empty, validate_payload};

const PALETTE_SIZE: i64 = 32;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarBulkImportPayload {
    target_calendar_id: String,
    now: String,
    #[serde(default)]
    preservation: Option<CalendarImportPreservation>,
    events: Vec<CalendarImportEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportPreservation {
    source_kind: String,
    source_name: String,
    source_fingerprint: String,
    objects: Vec<CalendarImportPreservedObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportPreservedObject {
    id: String,
    prodid: Option<String>,
    version: Option<String>,
    method: Option<String>,
    calendar_scale: Option<String>,
    raw_jcal: String,
    diagnostics: String,
    components: Vec<CalendarImportPreservedComponent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportPreservedComponent {
    id: String,
    component_type: String,
    uid: Option<String>,
    recurrence_id: Option<String>,
    recurrence_id_value_type: Option<String>,
    sequence: Option<i64>,
    dtstart_key: Option<String>,
    raw_jcal: String,
    preservation_status: String,
    projection_warnings: String,
    components: Vec<CalendarImportPreservedComponent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportEvent {
    candidate_id: String,
    icalendar_component_id: Option<String>,
    title: String,
    start_time: String,
    end_time: String,
    timezone: String,
    color: Option<i64>,
    description: String,
    rrule: Option<String>,
    notifications: Option<String>,
    exceptions: Option<String>,
    repeat_until: Option<String>,
    all_day: bool,
    location: String,
    url: String,
    transparency: String,
    status: String,
    source_uid: Option<String>,
    visibility: String,
    priority: Option<i64>,
    categories: Option<String>,
    geo: Option<String>,
    sequence: i64,
    rdate: Option<String>,
    extended_properties: Option<String>,
    organizer: Option<String>,
    meeting_enabled: bool,
    guest_can_modify: bool,
    guest_can_invite_others: bool,
    guest_can_see_other_guests: bool,
    attendees: Vec<CalendarImportAttendee>,
    alarms: Vec<CalendarImportAlarm>,
    overrides: Vec<CalendarImportOverride>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportAttendee {
    id: String,
    icalendar_component_id: Option<String>,
    icalendar_property_index: Option<i64>,
    name: Option<String>,
    email: String,
    role: String,
    status: String,
    rsvp: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportAlarm {
    id: String,
    icalendar_component_id: Option<String>,
    action: String,
    trigger_type: String,
    trigger_value: String,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarImportOverride {
    id: String,
    icalendar_component_id: Option<String>,
    recurrence_id: String,
    recurrence_range: Option<String>,
    title: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    description: Option<String>,
    location: Option<String>,
    url: Option<String>,
    color: Option<i64>,
    status: Option<String>,
    transparency: Option<String>,
    visibility: Option<String>,
    extended_properties: Option<String>,
}

#[derive(Deserialize)]
struct CalendarGeoPayload {
    lat: f64,
    lng: f64,
}

#[derive(Deserialize)]
struct CalendarOrganizerPayload {
    name: Option<String>,
    email: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarBulkImportSummary {
    added: usize,
    updated: usize,
    skipped_older: usize,
    warnings: Vec<String>,
    applied: Vec<CalendarImportApplied>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarImportApplied {
    candidate_id: String,
    event_id: String,
    action: CalendarImportAction,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum CalendarImportAction {
    Added,
    Updated,
}

struct ExistingEventRow {
    id: String,
    sequence: i64,
}

/// Validates and imports iCalendar events into their target calendar in one transaction.
pub async fn bulk_import(
    pool: &SqlitePool,
    payload: CalendarBulkImportPayload,
) -> Result<CalendarBulkImportSummary, String> {
    validate_payload(&payload)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let existing = load_existing_events(&mut tx, &payload).await?;
    let mut seen_source_uids = HashSet::new();
    let mut skipped_older = 0;
    let mut warnings = Vec::new();
    let mut applied = Vec::new();
    let apply_preservation_links =
        payload.preservation.is_some() && !contains_older_revisions(&existing, &payload.events);

    if let Some(preservation) = &payload.preservation {
        if apply_preservation_links {
            replace_preservation(
                &mut tx,
                &payload.target_calendar_id,
                &payload.now,
                preservation,
            )
            .await?;
        } else {
            warnings.push(
                "iCalendar preservation was not replaced because the import contains older event revisions."
                    .to_string(),
            );
        }
    }

    for event in &payload.events {
        let Some(source_uid) = event
            .source_uid
            .as_deref()
            .filter(|uid| !uid.trim().is_empty())
        else {
            warnings.push("Event without UID skipped.".to_string());
            continue;
        };
        if !seen_source_uids.insert(source_uid.to_string()) {
            warnings.push("Duplicate UID skipped.".to_string());
            continue;
        }

        if let Some(existing_row) = existing.get(source_uid) {
            if event.sequence < existing_row.sequence {
                skipped_older += 1;
                continue;
            }
            update_event(
                &mut tx,
                &payload.target_calendar_id,
                &payload.now,
                event,
                &existing_row.id,
                apply_preservation_links,
            )
            .await?;
            replace_child_rows(
                &mut tx,
                &existing_row.id,
                event,
                &payload.now,
                apply_preservation_links,
            )
            .await?;
            applied.push(CalendarImportApplied {
                candidate_id: event.candidate_id.clone(),
                event_id: existing_row.id.clone(),
                action: CalendarImportAction::Updated,
            });
        } else {
            insert_event(
                &mut tx,
                &payload.target_calendar_id,
                &payload.now,
                event,
                apply_preservation_links,
            )
            .await?;
            insert_child_rows(
                &mut tx,
                &event.candidate_id,
                event,
                &payload.now,
                apply_preservation_links,
            )
            .await?;
            applied.push(CalendarImportApplied {
                candidate_id: event.candidate_id.clone(),
                event_id: event.candidate_id.clone(),
                action: CalendarImportAction::Added,
            });
        }
    }

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let added = applied
        .iter()
        .filter(|event| matches!(event.action, CalendarImportAction::Added))
        .count();
    let updated = applied.len() - added;
    Ok(CalendarBulkImportSummary {
        added,
        updated,
        skipped_older,
        warnings,
        applied,
    })
}

async fn load_existing_events(
    tx: &mut Transaction<'_, Sqlite>,
    payload: &CalendarBulkImportPayload,
) -> Result<HashMap<String, ExistingEventRow>, String> {
    let source_uids: Vec<&str> = payload
        .events
        .iter()
        .filter_map(|event| event.source_uid.as_deref())
        .filter(|uid| !uid.trim().is_empty())
        .collect();
    let mut existing = HashMap::new();
    for chunk in source_uids.chunks(500) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let query = format!(
            "SELECT id, source_uid, sequence FROM calendar_events
             WHERE calendar_id = ? AND source_uid IN ({placeholders})"
        );
        let mut q = sqlx::query(&query).bind(&payload.target_calendar_id);
        for uid in chunk {
            q = q.bind(*uid);
        }
        let rows = q
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("load existing import rows: {e}"))?;
        for row in rows {
            let source_uid: String = row
                .try_get("source_uid")
                .map_err(|e| format!("read existing source_uid: {e}"))?;
            let sequence: Option<i64> = row
                .try_get("sequence")
                .map_err(|e| format!("read existing sequence: {e}"))?;
            existing.insert(
                source_uid,
                ExistingEventRow {
                    id: row
                        .try_get("id")
                        .map_err(|e| format!("read existing id: {e}"))?,
                    sequence: sequence.unwrap_or(0),
                },
            );
        }
    }
    Ok(existing)
}

fn contains_older_revisions(
    existing: &HashMap<String, ExistingEventRow>,
    events: &[CalendarImportEvent],
) -> bool {
    events.iter().any(|event| {
        event
            .source_uid
            .as_deref()
            .and_then(|uid| existing.get(uid))
            .is_some_and(|row| event.sequence < row.sequence)
    })
}

async fn insert_event(
    tx: &mut Transaction<'_, Sqlite>,
    target_calendar_id: &str,
    now: &str,
    event: &CalendarImportEvent,
    apply_preservation_links: bool,
) -> Result<(), String> {
    let description = sanitize_calendar_description_html(&event.description);
    let geo = parse_geo(&event.geo)?;
    let icalendar_component_id =
        preservation_link_id(&event.icalendar_component_id, apply_preservation_links);
    sqlx::query(
        "INSERT INTO calendar_events
            (id, title, start_time, end_time, timezone, calendar_id,
             color, description, rrule, repeat_until,
             all_day, location, url, transparency, status,
             source_uid, visibility, priority, geo_lat, geo_lng,
             sequence,
             meeting_enabled,
             guest_can_modify, guest_can_invite_others, guest_can_see_other_guests,
             icalendar_component_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.candidate_id)
    .bind(&event.title)
    .bind(&event.start_time)
    .bind(&event.end_time)
    .bind(&event.timezone)
    .bind(target_calendar_id)
    .bind(event.color)
    .bind(&description)
    .bind(&event.rrule)
    .bind(&event.repeat_until)
    .bind(if event.all_day { 1_i64 } else { 0_i64 })
    .bind(&event.location)
    .bind(&event.url)
    .bind(&event.transparency)
    .bind(&event.status)
    .bind(&event.source_uid)
    .bind(&event.visibility)
    .bind(event.priority)
    .bind(geo.map(|value| value.0))
    .bind(geo.map(|value| value.1))
    .bind(event.sequence)
    .bind(if event.meeting_enabled { 1_i64 } else { 0_i64 })
    .bind(if event.guest_can_modify { 1_i64 } else { 0_i64 })
    .bind(if event.guest_can_invite_others {
        1_i64
    } else {
        0_i64
    })
    .bind(if event.guest_can_see_other_guests {
        1_i64
    } else {
        0_i64
    })
    .bind(icalendar_component_id)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert imported event: {e}"))?;
    replace_import_event_metadata(tx, &event.candidate_id, event).await?;
    link_preserved_component(
        tx,
        icalendar_component_id,
        "event",
        &event.candidate_id,
        now,
    )
    .await?;
    Ok(())
}

async fn update_event(
    tx: &mut Transaction<'_, Sqlite>,
    target_calendar_id: &str,
    now: &str,
    event: &CalendarImportEvent,
    event_id: &str,
    apply_preservation_links: bool,
) -> Result<(), String> {
    let description = sanitize_calendar_description_html(&event.description);
    let geo = parse_geo(&event.geo)?;
    let icalendar_component_id =
        preservation_link_id(&event.icalendar_component_id, apply_preservation_links);
    let result = sqlx::query(
        "UPDATE calendar_events
            SET title = ?, start_time = ?, end_time = ?, timezone = ?,
                color = ?, description = ?,
                rrule = ?, repeat_until = ?,
                all_day = ?, location = ?, url = ?,
                transparency = ?, status = ?,
                visibility = ?, priority = ?,
                geo_lat = ?, geo_lng = ?,
                sequence = ?,
                meeting_enabled = ?,
                guest_can_modify = ?, guest_can_invite_others = ?,
                guest_can_see_other_guests = ?,
                icalendar_component_id = CASE WHEN ? = 1 THEN ? ELSE icalendar_component_id END,
                updated_at = ?
          WHERE id = ? AND calendar_id = ?",
    )
    .bind(&event.title)
    .bind(&event.start_time)
    .bind(&event.end_time)
    .bind(&event.timezone)
    .bind(event.color)
    .bind(&description)
    .bind(&event.rrule)
    .bind(&event.repeat_until)
    .bind(if event.all_day { 1_i64 } else { 0_i64 })
    .bind(&event.location)
    .bind(&event.url)
    .bind(&event.transparency)
    .bind(&event.status)
    .bind(&event.visibility)
    .bind(event.priority)
    .bind(geo.map(|value| value.0))
    .bind(geo.map(|value| value.1))
    .bind(event.sequence)
    .bind(if event.meeting_enabled { 1_i64 } else { 0_i64 })
    .bind(if event.guest_can_modify { 1_i64 } else { 0_i64 })
    .bind(if event.guest_can_invite_others {
        1_i64
    } else {
        0_i64
    })
    .bind(if event.guest_can_see_other_guests {
        1_i64
    } else {
        0_i64
    })
    .bind(if apply_preservation_links {
        1_i64
    } else {
        0_i64
    })
    .bind(icalendar_component_id)
    .bind(now)
    .bind(event_id)
    .bind(target_calendar_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("update imported event: {e}"))?;
    if result.rows_affected() == 0 {
        return Err(format!("import target event '{event_id}' not found"));
    }
    replace_import_event_metadata(tx, event_id, event).await?;
    link_preserved_component(tx, icalendar_component_id, "event", event_id, now).await?;
    Ok(())
}

async fn replace_import_event_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    event: &CalendarImportEvent,
) -> Result<(), String> {
    replace_i64_list(
        tx,
        event_id,
        "calendar_event_notifications",
        "offset_minutes",
        parse_i64_list(&event.notifications, "notifications")?,
    )
    .await?;
    replace_string_list(
        tx,
        event_id,
        "calendar_event_exdates",
        "occurrence_date",
        parse_string_list(&event.exceptions, "exceptions")?,
    )
    .await?;
    replace_string_list(
        tx,
        event_id,
        "calendar_event_categories",
        "category",
        parse_string_list(&event.categories, "categories")?,
    )
    .await?;
    replace_string_list(
        tx,
        event_id,
        "calendar_event_rdates",
        "occurrence_start",
        parse_string_list(&event.rdate, "rdate")?,
    )
    .await?;
    replace_extended_properties(
        tx,
        event_id,
        "calendar_event_extended_properties",
        "event_id",
        parse_string_map(&event.extended_properties, "extended_properties")?,
    )
    .await?;
    replace_organizer(tx, event_id, parse_organizer(&event.organizer)?).await
}

async fn replace_child_rows(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    event: &CalendarImportEvent,
    now: &str,
    apply_preservation_links: bool,
) -> Result<(), String> {
    for query in [
        "DELETE FROM calendar_event_attendees WHERE event_id = ?",
        "DELETE FROM calendar_event_alarms WHERE event_id = ?",
        "DELETE FROM calendar_event_overrides WHERE parent_event_id = ?",
    ] {
        sqlx::query(query)
            .bind(event_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("delete imported event children: {e}"))?;
    }
    insert_child_rows(tx, event_id, event, now, apply_preservation_links).await
}

async fn insert_child_rows(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    event: &CalendarImportEvent,
    now: &str,
    apply_preservation_links: bool,
) -> Result<(), String> {
    for (sort_order, attendee) in event.attendees.iter().enumerate() {
        let icalendar_component_id =
            preservation_link_id(&attendee.icalendar_component_id, apply_preservation_links);
        sqlx::query(
            "INSERT INTO calendar_event_attendees
                (id, event_id, icalendar_component_id, icalendar_property_index,
                 name, email, role, status, rsvp, sort_order)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&attendee.id)
        .bind(event_id)
        .bind(icalendar_component_id)
        .bind(if apply_preservation_links {
            attendee.icalendar_property_index
        } else {
            None
        })
        .bind(&attendee.name)
        .bind(&attendee.email)
        .bind(&attendee.role)
        .bind(&attendee.status)
        .bind(if attendee.rsvp { 1_i64 } else { 0_i64 })
        .bind(sort_order as i64)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert imported attendee: {e}"))?;
    }

    for (sort_order, alarm) in event.alarms.iter().enumerate() {
        let icalendar_component_id =
            preservation_link_id(&alarm.icalendar_component_id, apply_preservation_links);
        sqlx::query(
            "INSERT INTO calendar_event_alarms
                (id, event_id, icalendar_component_id, action, trigger_type,
                 trigger_value, description, sort_order)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&alarm.id)
        .bind(event_id)
        .bind(icalendar_component_id)
        .bind(&alarm.action)
        .bind(&alarm.trigger_type)
        .bind(&alarm.trigger_value)
        .bind(&alarm.description)
        .bind(sort_order as i64)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert imported alarm: {e}"))?;
        link_preserved_component(tx, icalendar_component_id, "alarm", &alarm.id, now).await?;
    }

    for override_row in &event.overrides {
        let description = sanitize_optional_calendar_description(&override_row.description);
        let icalendar_component_id = preservation_link_id(
            &override_row.icalendar_component_id,
            apply_preservation_links,
        );
        sqlx::query(
            "INSERT INTO calendar_event_overrides
                (id, parent_event_id, recurrence_id, recurrence_range, title, start_time, end_time,
                 description, location, url, color, status, transparency, visibility,
                 icalendar_component_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&override_row.id)
        .bind(event_id)
        .bind(&override_row.recurrence_id)
        .bind(&override_row.recurrence_range)
        .bind(&override_row.title)
        .bind(&override_row.start_time)
        .bind(&override_row.end_time)
        .bind(&description)
        .bind(&override_row.location)
        .bind(&override_row.url)
        .bind(override_row.color)
        .bind(&override_row.status)
        .bind(&override_row.transparency)
        .bind(&override_row.visibility)
        .bind(icalendar_component_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert imported override: {e}"))?;
        replace_extended_properties(
            tx,
            &override_row.id,
            "calendar_event_override_extended_properties",
            "override_id",
            parse_string_map(
                &override_row.extended_properties,
                "override.extended_properties",
            )?,
        )
        .await?;
        link_preserved_component(
            tx,
            icalendar_component_id,
            "override",
            &override_row.id,
            now,
        )
        .await?;
    }
    Ok(())
}

fn parse_i64_list(value: &Option<String>, field: &str) -> Result<Vec<i64>, String> {
    match value {
        Some(json) => serde_json::from_str::<Vec<i64>>(json)
            .map_err(|e| format!("{field} is not a valid integer list: {e}")),
        None => Ok(Vec::new()),
    }
}

fn parse_string_list(value: &Option<String>, field: &str) -> Result<Vec<String>, String> {
    match value {
        Some(json) => serde_json::from_str::<Vec<String>>(json)
            .map_err(|e| format!("{field} is not a valid string list: {e}")),
        None => Ok(Vec::new()),
    }
}

fn parse_string_map(value: &Option<String>, field: &str) -> Result<Vec<(String, String)>, String> {
    let Some(json) = value else {
        return Ok(Vec::new());
    };
    let map = serde_json::from_str::<std::collections::BTreeMap<String, String>>(json)
        .map_err(|e| format!("{field} is not a valid string map: {e}"))?;
    Ok(map.into_iter().collect())
}

fn parse_geo(value: &Option<String>) -> Result<Option<(f64, f64)>, String> {
    let Some(json) = value else {
        return Ok(None);
    };
    let geo = serde_json::from_str::<CalendarGeoPayload>(json)
        .map_err(|e| format!("geo is not valid coordinates: {e}"))?;
    if !geo.lat.is_finite() || !geo.lng.is_finite() {
        return Err("geo coordinates must be finite".to_string());
    }
    Ok(Some((geo.lat, geo.lng)))
}

fn parse_organizer(value: &Option<String>) -> Result<Option<CalendarOrganizerPayload>, String> {
    let Some(json) = value else {
        return Ok(None);
    };
    let organizer = serde_json::from_str::<CalendarOrganizerPayload>(json)
        .map_err(|e| format!("organizer is not valid: {e}"))?;
    require_non_empty(&organizer.email, "organizer.email")?;
    Ok(Some(organizer))
}

async fn replace_i64_list(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    table: &'static str,
    column: &'static str,
    values: Vec<i64>,
) -> Result<(), String> {
    let delete = format!("DELETE FROM {table} WHERE event_id = ?");
    sqlx::query(&delete)
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar {table}: {e}"))?;
    let insert = format!(
        "INSERT INTO {table} (id, event_id, {column}, sort_order)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?)"
    );
    for (sort_order, value) in values.into_iter().enumerate() {
        sqlx::query(&insert)
            .bind(event_id)
            .bind(value)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert calendar {table}: {e}"))?;
    }
    Ok(())
}

async fn replace_string_list(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    table: &'static str,
    column: &'static str,
    values: Vec<String>,
) -> Result<(), String> {
    let delete = format!("DELETE FROM {table} WHERE event_id = ?");
    sqlx::query(&delete)
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar {table}: {e}"))?;
    let insert = format!(
        "INSERT INTO {table} (id, event_id, {column}, sort_order)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?)"
    );
    for (sort_order, value) in values.into_iter().enumerate() {
        sqlx::query(&insert)
            .bind(event_id)
            .bind(value)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert calendar {table}: {e}"))?;
    }
    Ok(())
}

async fn replace_extended_properties(
    tx: &mut Transaction<'_, Sqlite>,
    owner_id: &str,
    table: &'static str,
    owner_column: &'static str,
    values: Vec<(String, String)>,
) -> Result<(), String> {
    let delete = format!("DELETE FROM {table} WHERE {owner_column} = ?");
    sqlx::query(&delete)
        .bind(owner_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar {table}: {e}"))?;
    let insert = format!(
        "INSERT INTO {table} (id, {owner_column}, property_key, property_value, sort_order)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?)"
    );
    for (sort_order, (key, value)) in values.into_iter().enumerate() {
        sqlx::query(&insert)
            .bind(owner_id)
            .bind(key)
            .bind(value)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert calendar {table}: {e}"))?;
    }
    Ok(())
}

async fn replace_organizer(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    organizer: Option<CalendarOrganizerPayload>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM calendar_event_organizers WHERE event_id = ?")
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar organizer: {e}"))?;
    if let Some(organizer) = organizer {
        sqlx::query(
            "INSERT INTO calendar_event_organizers (event_id, name, email)
             VALUES (?, ?, ?)",
        )
        .bind(event_id)
        .bind(organizer.name)
        .bind(organizer.email)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert calendar organizer: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::preservation::replace_preservation;
    use super::validation::{
        validate_color, validate_enum, validate_event, validate_json_option, validate_preservation,
        validate_priority,
    };
    use super::{
        CalendarImportAlarm, CalendarImportAttendee, CalendarImportEvent, CalendarImportOverride,
        CalendarImportPreservation, CalendarImportPreservedComponent,
        CalendarImportPreservedObject, ExistingEventRow, contains_older_revisions,
        insert_child_rows, insert_event, update_event,
    };

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

    #[test]
    fn stores_icalendar_preserved_objects_and_components() {
        crate::test_support::block_on(async {
            let pool = in_memory_pool().await;
            let mut tx = pool.begin().await.unwrap();
            let preservation = preservation_payload("source.ics");

            replace_preservation(&mut tx, "local", "2026-05-09 10:00:00", &preservation)
                .await
                .unwrap();
            tx.commit().await.unwrap();

            let object_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
                .fetch_one(&pool)
                .await
                .unwrap();
            let component_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_components")
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

            replace_preservation(&mut tx, "local", "2026-05-09 10:00:00", &preservation)
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

            replace_preservation(&mut tx, "local", "2026-05-09 10:00:00", &first)
                .await
                .unwrap();
            replace_preservation(&mut tx, "local", "2026-05-09 10:05:00", &second)
                .await
                .unwrap();
            tx.commit().await.unwrap();

            let ids: Vec<String> =
                sqlx::query_scalar("SELECT id FROM icalendar_objects ORDER BY id")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(ids, vec!["object-2".to_string()]);
        });
    }

    #[test]
    fn linked_import_rows_reference_preserved_components() {
        crate::test_support::block_on(async {
            let pool = in_memory_pool().await;
            let mut tx = pool.begin().await.unwrap();
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

            replace_preservation(&mut tx, "local", "2026-05-09 10:00:00", &preservation)
                .await
                .unwrap();
            insert_event(&mut tx, "local", "2026-05-09 10:00:00", &event, true)
                .await
                .unwrap();
            insert_child_rows(&mut tx, "event-1", &event, "2026-05-09 10:00:00", true)
                .await
                .unwrap();
            tx.commit().await.unwrap();

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

            replace_preservation(&mut tx, "local", "2026-05-09 10:00:00", &preservation)
                .await
                .unwrap();
            insert_event(&mut tx, "local", "2026-05-09 10:00:00", &initial, true)
                .await
                .unwrap();
            update_event(
                &mut tx,
                "local",
                "2026-05-09 10:05:00",
                &older,
                "event-1",
                false,
            )
            .await
            .unwrap();
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

            insert_event(&mut tx, "local", "2026-05-09 10:00:00", &event, false)
                .await
                .unwrap();
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

            insert_event(&mut tx, "local", "2026-05-09 10:00:00", &initial, false)
                .await
                .unwrap();
            update_event(
                &mut tx,
                "local",
                "2026-05-09 10:05:00",
                &updated,
                "event-1",
                false,
            )
            .await
            .unwrap();
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

            insert_event(&mut tx, "local", "2026-05-09 10:00:00", &event, false)
                .await
                .unwrap();
            insert_child_rows(&mut tx, "event-1", &event, "2026-05-09 10:00:00", false)
                .await
                .unwrap();
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

    async fn in_memory_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::run_migrations(&pool).await.unwrap();
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
}
