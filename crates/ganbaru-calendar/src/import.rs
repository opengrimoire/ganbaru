//! iCalendar bulk import into one calendar, with revision ordering and preservation links.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Connection, Row, Sqlite, SqliteConnection, SqlitePool, Transaction};

mod diff;
mod preservation;
mod validation;

use diff::{EventPlan, LinkMode, plan_event, write_event};
use preservation::{PreservationOutcome, ProjectedRow, preservation_link_id, replace_preservation};
use validation::{require_non_empty, skip_reason, timing_refusal, validate_payload, value_refusal};

const PALETTE_SIZE: i64 = 32;

/// Deepest preserved iCalendar component tree, counting the `VCALENDAR` root as the first level.
///
/// Import rejects deeper trees, and export reconstruction enforces the same limit.
pub(crate) const MAX_ICALENDAR_COMPONENT_DEPTH: usize = 32;
/// Total bytes of preserved component jCal one import may store; each component carries only its own
/// properties, so this stays proportional to the source file.
const MAX_PRESERVED_JCAL_BYTES: usize = 64 * 1024 * 1024;
/// Most events one import may carry, five times the 50,000 components the frontend parser accepts
/// from one file.
const MAX_IMPORT_EVENTS: usize = 250_000;
/// Most attendees one imported event may carry before it is skipped with a warning.
const MAX_IMPORT_EVENT_ATTENDEES: usize = 10_000;
/// Most alarms one imported event may carry before it is skipped with a warning.
const MAX_IMPORT_EVENT_ALARMS: usize = 1_000;
/// Most overrides one imported recurring event may carry before it is skipped with a warning.
const MAX_IMPORT_EVENT_OVERRIDES: usize = 50_000;
/// Most excluded or added dates one imported event may list, each, before it is skipped with a
/// warning.
const MAX_IMPORT_EVENT_DATES: usize = 50_000;

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
    let mut conn = pool.acquire().await.map_err(|e| format!("acquire: {e}"))?;
    bulk_import_on(&mut conn, payload).await
}

/// Imports like [`bulk_import`] on one connection, in a transaction of its own.
///
/// Each event is written as a diff against its stored rows: rows whose content the source still
/// carries keep their ids, and an event whose rows all match is not written. Re-importing the
/// source its preserved components came from, unchanged, keeps those components and writes
/// nothing.
pub async fn bulk_import_on(
    conn: &mut SqliteConnection,
    payload: CalendarBulkImportPayload,
) -> Result<CalendarBulkImportSummary, String> {
    validate_payload(&payload)?;
    let mut tx = conn.begin().await.map_err(|e| format!("begin: {e}"))?;
    let existing = load_existing_events(&mut tx, &payload).await?;
    let apply_preservation_links =
        payload.preservation.is_some() && !contains_older_revisions(&existing, &payload.events);
    let plan = plan_events(&payload.events, &existing);
    let skipped_older = plan.skipped_older;
    let mut warnings = Vec::new();

    let mut writes = plan_writes(&mut tx, &payload, &plan, LinkMode::Keep).await?;
    if let Some(preservation) = &payload.preservation {
        if !apply_preservation_links {
            warnings.push(
                "iCalendar preservation was not replaced because the import contains older event revisions."
                    .to_string(),
            );
        } else if !preservation_is_current(&mut tx, &payload, preservation, &writes).await? {
            writes = plan_writes(&mut tx, &payload, &plan, LinkMode::Relink).await?;
            let outcome = preservation_outcome(preservation, &plan, &writes);
            replace_preservation(
                &mut tx,
                &payload.target_calendar_id,
                &payload.now,
                preservation,
                &outcome,
            )
            .await?;
        }
    }

    warnings.extend(plan.warnings.iter().cloned());

    let mut applied = Vec::new();
    for ((event, decision), write) in plan.decisions.iter().zip(writes) {
        let (event_id, action) = match decision {
            EventDecision::Update(row) => (row.id.clone(), CalendarImportAction::Updated),
            EventDecision::Insert => (event.candidate_id.clone(), CalendarImportAction::Added),
        };
        write_event(
            &mut tx,
            &payload.target_calendar_id,
            &payload.now,
            event,
            write,
        )
        .await?;
        applied.push(CalendarImportApplied {
            candidate_id: event.candidate_id.clone(),
            event_id,
            action,
        });
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

/// Plans the writes of every decided event, in decision order.
async fn plan_writes(
    conn: &mut SqliteConnection,
    payload: &CalendarBulkImportPayload,
    plan: &ImportPlan<'_>,
    mode: LinkMode,
) -> Result<Vec<EventPlan>, String> {
    let mut writes = Vec::with_capacity(plan.decisions.len());
    for (event, decision) in &plan.decisions {
        let (event_id, stored) = match decision {
            EventDecision::Update(row) => (row.id.as_str(), true),
            EventDecision::Insert => (event.candidate_id.as_str(), false),
        };
        writes.push(
            plan_event(
                &mut *conn,
                &payload.target_calendar_id,
                event_id,
                stored,
                event,
                mode,
                &payload.now,
            )
            .await?,
        );
    }
    Ok(writes)
}

/// Whether the stored preserved components already hold this source: the import adds no event,
/// changes no row, and every stored object of the source carries the source's fingerprint.
async fn preservation_is_current(
    conn: &mut SqliteConnection,
    payload: &CalendarBulkImportPayload,
    preservation: &CalendarImportPreservation,
    writes: &[EventPlan],
) -> Result<bool, String> {
    if writes
        .iter()
        .any(|write| write.inserts() || write.changes())
    {
        return Ok(false);
    }
    let fingerprints: Vec<String> = sqlx::query_scalar(
        "SELECT source_fingerprint FROM icalendar_objects
         WHERE calendar_id = ? AND source_kind = ? AND source_name = ?",
    )
    .bind(&payload.target_calendar_id)
    .bind(&preservation.source_kind)
    .bind(&preservation.source_name)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| format!("load stored iCalendar preservation: {e}"))?;
    Ok(fingerprints.len() == preservation.objects.len()
        && fingerprints
            .iter()
            .all(|fingerprint| *fingerprint == preservation.source_fingerprint))
}

/// What one importable event does to the vault.
enum EventDecision<'a> {
    /// The event replaces the stored row with this identity.
    Update(&'a ExistingEventRow),
    /// The event becomes a new row under its candidate identity.
    Insert,
}

/// The per-event outcome of an import, decided before any row is written so preserved components
/// can be stored with their projection.
struct ImportPlan<'a> {
    /// Events that are written, in payload order.
    decisions: Vec<(&'a CalendarImportEvent, EventDecision<'a>)>,
    /// Warnings for skipped events, in payload order.
    warnings: Vec<String>,
    /// Events left alone because the stored revision is newer.
    skipped_older: usize,
    /// Events refused because their timing value is over the sync bound.
    oversized: Vec<(&'a CalendarImportEvent, String)>,
}

fn plan_events<'a>(
    events: &'a [CalendarImportEvent],
    existing: &'a HashMap<String, ExistingEventRow>,
) -> ImportPlan<'a> {
    let mut plan = ImportPlan {
        decisions: Vec::new(),
        warnings: Vec::new(),
        skipped_older: 0,
        oversized: Vec::new(),
    };
    let mut seen_source_uids = HashSet::new();
    for event in events {
        if let Some(reason) = skip_reason(event) {
            plan.warnings.push(format!("Event skipped: {reason}."));
            continue;
        }
        if let Some(refusal) = timing_refusal(event).or_else(|| value_refusal(event)) {
            plan.warnings.push(refusal.clone());
            plan.oversized.push((event, refusal));
            continue;
        }
        let Some(source_uid) = event
            .source_uid
            .as_deref()
            .filter(|uid| !uid.trim().is_empty())
        else {
            plan.warnings.push("Event without UID skipped.".to_string());
            continue;
        };
        if !seen_source_uids.insert(source_uid.to_string()) {
            plan.warnings.push("Duplicate UID skipped.".to_string());
            continue;
        }
        match existing.get(source_uid) {
            Some(row) if event.sequence < row.sequence => plan.skipped_older += 1,
            Some(row) => plan.decisions.push((event, EventDecision::Update(row))),
            None => plan.decisions.push((event, EventDecision::Insert)),
        }
    }
    plan
}

/// Projects every written event, alarm, and override onto its preserved component, and names each
/// refused event in the diagnostics of the object that holds its component.
fn preservation_outcome(
    preservation: &CalendarImportPreservation,
    plan: &ImportPlan<'_>,
    writes: &[EventPlan],
) -> PreservationOutcome {
    let mut outcome = PreservationOutcome::default();
    for ((event, decision), write) in plan.decisions.iter().zip(writes) {
        let event_id = match decision {
            EventDecision::Update(row) => row.id.as_str(),
            EventDecision::Insert => event.candidate_id.as_str(),
        };
        let mut project = |component: &Option<String>, kind: &'static str, id: &str| {
            if let Some(component_id) = preservation_link_id(component, true) {
                outcome.projections.insert(
                    component_id.to_string(),
                    ProjectedRow {
                        kind,
                        id: id.to_string(),
                    },
                );
            }
        };
        project(&event.icalendar_component_id, "event", event_id);
        for alarm in &event.alarms {
            project(
                &alarm.icalendar_component_id,
                "alarm",
                write.row_id(&alarm.id),
            );
        }
        for override_row in &event.overrides {
            project(
                &override_row.icalendar_component_id,
                "override",
                write.row_id(&override_row.id),
            );
        }
    }
    if !plan.oversized.is_empty() {
        let owners = component_objects(preservation);
        for (event, message) in &plan.oversized {
            let owner = preservation_link_id(&event.icalendar_component_id, true)
                .and_then(|component_id| owners.get(component_id));
            if let Some(object_id) = owner {
                outcome
                    .diagnostics
                    .entry((*object_id).to_string())
                    .or_default()
                    .push(message.clone());
            }
        }
    }
    outcome
}

/// Maps every preserved component id to the id of the object that holds it.
fn component_objects(preservation: &CalendarImportPreservation) -> HashMap<&str, &str> {
    let mut owners = HashMap::new();
    for object in &preservation.objects {
        let mut pending: Vec<&CalendarImportPreservedComponent> =
            object.components.iter().collect();
        while let Some(component) = pending.pop() {
            owners.insert(component.id.as_str(), object.id.as_str());
            pending.extend(component.components.iter());
        }
    }
    owners
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
        let mut q = sqlx::query(AssertSqlSafe(query.as_str())).bind(&payload.target_calendar_id);
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

fn parse_organizer(value: &Option<String>) -> Result<Option<CalendarOrganizerPayload>, String> {
    let Some(json) = value else {
        return Ok(None);
    };
    let organizer = serde_json::from_str::<CalendarOrganizerPayload>(json)
        .map_err(|e| format!("organizer is not valid: {e}"))?;
    require_non_empty(&organizer.email, "organizer.email")?;
    Ok(Some(organizer))
}

/// Whether a list table is a date set keyed by its date, which repeats at most once and has no
/// stored order.
#[cfg(test)]
pub(crate) fn is_date_set(table: &str) -> bool {
    matches!(table, "calendar_event_exdates" | "calendar_event_rdates")
}

/// Insert statement for one value of a list table. A date set ignores a repeated date.
#[cfg(test)]
pub(crate) fn string_list_insert(table: &str, column: &str) -> String {
    if is_date_set(table) {
        format!(
            "INSERT INTO {table} (id, event_id, {column})
             VALUES (lower(hex(randomblob(16))), ?, ?) ON CONFLICT DO NOTHING"
        )
    } else {
        format!(
            "INSERT INTO {table} (id, event_id, {column}, sort_order)
             VALUES (lower(hex(randomblob(16))), ?, ?, ?)"
        )
    }
}

#[cfg(test)]
mod tests;
