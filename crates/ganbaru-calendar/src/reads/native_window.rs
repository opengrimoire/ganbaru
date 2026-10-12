//! One consistent native Calendar read and home-zone recurrence projection.

use ganbaru_db::impl_sqlite_from_row;
use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use super::export::ExportBudget;
use super::{DbCalendarEventRow, DbOverrideRow, DbWindowAttendeeRow};
use crate::recurrence::canonical::{
    StoredOverride, StoredTemplate, Template, Window, expand_templates,
};
use crate::work_gate::WorkGate;
use ganbaru_civil_time as civil_time;

const MAX_TEMPLATES: usize = 10_000;
const MAX_ID_BYTES: usize = 1_024;
const SECONDS_PER_DAY: f64 = 86_400.0;
static WINDOW_GATE: WorkGate = WorkGate::new();

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeWindowRequest {
    pub window_start_date: String,
    pub window_end_date: String,
    pub render_zone: String,
    #[serde(default)]
    pub include_total_event_count: bool,
}

#[derive(Serialize)]
pub struct NativeOccurrence {
    pub template_id: String,
    pub id: String,
    pub recurring_parent_id: Option<String>,
    pub recurrence_date: String,
    pub start_time: String,
    pub end_time: String,
    pub override_id: Option<String>,
}

#[derive(Serialize)]
pub struct ExpansionDiagnostic {
    pub event_id: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct NativeCalendarWindow {
    pub events: Vec<DbCalendarEventRow>,
    pub overrides: Vec<DbOverrideRow>,
    pub attendees: Vec<DbWindowAttendeeRow>,
    pub total_event_count: Option<i64>,
    pub occurrences: Vec<NativeOccurrence>,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowPurpose {
    Render,
    Focus,
    Notifications,
    Music,
}

#[derive(Serialize)]
struct Selection {
    start_date: String,
    end_date: String,
    start_utc: String,
    end_exclusive_utc: String,
    minimum_home_offset_days: f64,
    maximum_home_offset_days: f64,
    purpose: WindowPurpose,
}

const EVENTS_SQL: &str = r#"
    SELECT ce.id, ce.title, ce.start_time, ce.end_time, ce.timezone,
           ce.calendar_id, ce.project_id, ce.environment_id, ce.playlist_id, ce.color, ce.rrule,
           NULL AS notifications, NULL AS exceptions, ce.repeat_until,
           ce.all_day, ce.location, ce.transparency, ce.status,
           CASE WHEN ce.url <> '' THEN 1 ELSE 0 END AS has_call_link,
           ce.meeting_enabled, ce.local_rsvp_status, ce.created_at,
           NULL AS rdate,
           pc.rhythm_kind, pc.rhythm_source, pc.preset_key,
           pcc.focus_duration_minutes AS count_focus_duration_minutes,
           pcc.short_break_minutes AS count_short_break_minutes,
           pcc.long_break_minutes AS count_long_break_minutes,
           pcc.long_break_after_focus_count AS count_long_break_after_focus_count,
           (SELECT '[' || group_concat(step_json) || ']' FROM (
             SELECT json_object('focusDurationMinutes', pcss.focus_duration_minutes,
               'breakPhase', pcss.break_phase, 'breakDurationMinutes', pcss.break_duration_minutes) AS step_json
             FROM calendar_event_pomodoro_config_sequence_steps pcss WHERE pcss.event_id = ce.id ORDER BY pcss.step_index
           )) AS sequence_steps,
           pc.idle_timeout_minutes
    FROM calendar_events ce
    LEFT JOIN calendar_event_pomodoro_configs pc ON pc.event_id = ce.id
    LEFT JOIN calendar_event_pomodoro_config_count_rhythms pcc ON pcc.event_id = ce.id
    WHERE (json_extract(?1, '$.purpose') <> 'focus' OR pc.event_id IS NOT NULL)
    AND (json_extract(?1, '$.purpose') <> 'music' OR
         (pc.event_id IS NULL AND ce.all_day = 0 AND ce.status <> 'cancelled'))
    AND (json_extract(?1, '$.purpose') <> 'notifications' OR EXISTS (
      SELECT 1 FROM calendar_event_notifications n WHERE n.event_id = ce.id
    )) AND (
      (ce.rrule IS NOT NULL AND ce.rrule <> '')
      OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
      OR EXISTS (SELECT 1 FROM calendar_event_overrides o WHERE o.parent_event_id = ce.id)
      OR (ce.all_day = 1 AND substr(ce.end_time, 1, 10) >= json_extract(?1, '$.start_date')
          AND substr(ce.start_time, 1, 10) <= json_extract(?1, '$.end_date'))
      OR (ce.all_day <> 1
          AND julianday(ce.end_time) >= julianday(json_extract(?1, '$.start_utc'))
              + json_extract(?1, '$.minimum_home_offset_days')
          AND julianday(ce.start_time) < julianday(json_extract(?1, '$.end_exclusive_utc'))
              + json_extract(?1, '$.maximum_home_offset_days'))
    )
    ORDER BY ce.start_time, ce.id
"#;

const EVENT_TEXT_COLUMNS: &[&str] = &[
    "id",
    "title",
    "start_time",
    "end_time",
    "timezone",
    "calendar_id",
    "project_id",
    "environment_id",
    "playlist_id",
    "rrule",
    "repeat_until",
    "location",
    "transparency",
    "status",
    "local_rsvp_status",
    "created_at",
    "rhythm_kind",
    "rhythm_source",
    "preset_key",
    "sequence_steps",
];

struct ProjectionField {
    event_id: String,
    field: String,
    value: String,
}
impl_sqlite_from_row!(ProjectionField {
    event_id,
    field,
    value
});

pub struct WindowSource {
    events: Vec<DbCalendarEventRow>,
    overrides: Vec<DbOverrideRow>,
    attendees: Vec<DbWindowAttendeeRow>,
    total_event_count: Option<i64>,
}

pub(crate) fn utc(epoch_ms: i64) -> Result<String, String> {
    DateTime::<Utc>::from_timestamp_millis(epoch_ms)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| "Calendar instant is outside its UTC range".into())
}

/// Authorize first, capture a read snapshot, and release SQLite before CPU expansion.
pub(super) async fn load(
    pool: &SqlitePool,
    request: NativeWindowRequest,
    purpose: WindowPurpose,
) -> Result<NativeCalendarWindow, String> {
    let permit = WINDOW_GATE
        .acquire()
        .await
        .map_err(|_| "The Calendar window gate is closed")?;
    let window = Window::new(
        &request.window_start_date,
        &request.window_end_date,
        &civil_time::zone(&request.render_zone)?,
    )?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin native Calendar window: {error}"))?;
    let source = read(&mut tx, &window, purpose, request.include_total_event_count).await?;
    tx.commit()
        .await
        .map_err(|error| format!("finish native Calendar window snapshot: {error}"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        source.expand(&window)
    })
    .await
    .map_err(|error| format!("native Calendar expansion worker: {error}"))?
}

/// Reuse the accepted transaction for Focus admission, with no pool re-entry.
pub async fn read(
    connection: &mut SqliteConnection,
    window: &Window,
    purpose: WindowPurpose,
    include_total: bool,
) -> Result<WindowSource, String> {
    let include_attendees = matches!(purpose, WindowPurpose::Render);
    // SQLite treats offset-free labels as UTC, whereas canonical expansion
    // resolves them in their home zone. This prefilter must admit every possible
    // supported offset; native expansion then applies the exact viewport.
    let selection = serde_json::to_string(&Selection {
        start_date: window.start_date.to_string(),
        end_date: window.end_date.to_string(),
        start_utc: utc(window.start_ms)?,
        end_exclusive_utc: utc(window.end_exclusive_ms)?,
        minimum_home_offset_days: f64::from(jiff::tz::Offset::MIN.seconds()) / SECONDS_PER_DAY,
        maximum_home_offset_days: f64::from(jiff::tz::Offset::MAX.seconds()) / SECONDS_PER_DAY,
        purpose,
    })
    .map_err(|error| error.to_string())?;
    let mut budget = ExportBudget::default();
    let mut events: Vec<DbCalendarEventRow> = budget
        .read(
            connection,
            &selection,
            &format!("SELECT * FROM ({EVENTS_SQL} LIMIT {})", MAX_TEMPLATES + 1),
            EVENT_TEXT_COLUMNS,
        )
        .await?;
    if events.len() > MAX_TEMPLATES {
        return Err(
            "Calendar window exceeds 10000 canonical templates; narrow or reduce recurring sources"
                .into(),
        );
    }
    let total_event_count = if include_total {
        Some(
            sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
                .fetch_one(&mut *connection)
                .await
                .map_err(|error| format!("count native Calendar events: {error}"))?,
        )
    } else {
        None
    };
    // Native Focus and Music poll empty windows too. No selected parent means
    // there are no children to read or statement plans to retain for this window.
    if events.is_empty() {
        return Ok(WindowSource {
            events,
            overrides: Vec::new(),
            attendees: Vec::new(),
            total_event_count,
        });
    }
    if events
        .iter()
        .any(|event| event.id.trim().is_empty() || event.id.len() > MAX_ID_BYTES)
    {
        return Err("Calendar template identity is empty or exceeds 1024 bytes".into());
    }
    let ids = serde_json::to_string(&events.iter().map(|event| &event.id).collect::<Vec<_>>())
        .map_err(|error| error.to_string())?;
    let overrides: Vec<DbOverrideRow> = budget.read(connection, &ids,
        "SELECT id, parent_event_id, recurrence_id, recurrence_range, title, start_time, end_time, color, status, transparency
         FROM calendar_event_overrides WHERE parent_event_id IN (SELECT value FROM json_each(?1))
         ORDER BY parent_event_id, recurrence_id, id",
        &["id", "parent_event_id", "recurrence_id", "recurrence_range", "title", "start_time", "end_time", "status", "transparency"],
    ).await?;
    if overrides
        .iter()
        .any(|row| row.id.trim().is_empty() || row.id.len() > MAX_ID_BYTES)
    {
        return Err("Calendar override identity is empty or exceeds 1024 bytes".into());
    }
    let attendees = if !include_attendees {
        Vec::new()
    } else {
        budget
            .read(
                connection,
                &ids,
                "SELECT event_id, email, status FROM calendar_event_attendees
             WHERE event_id IN (SELECT value FROM json_each(?1)) ORDER BY event_id, sort_order, id",
                &["event_id", "email", "status"],
            )
            .await?
    };
    let mut fields: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (table, field, column) in [
        (
            "calendar_event_notifications",
            "notifications",
            "offset_minutes",
        ),
        ("calendar_event_exdates", "exceptions", "occurrence_date"),
        ("calendar_event_rdates", "rdate", "occurrence_start"),
    ] {
        let rows: Vec<ProjectionField> = budget.read(connection, &ids,
            &format!("SELECT event_id, '{field}' AS field, CAST({column} AS TEXT) AS value FROM {table}
             WHERE event_id IN (SELECT value FROM json_each(?1)) ORDER BY event_id, sort_order, id"),
            &["event_id", "field", "value"],
        ).await?;
        for row in rows {
            fields
                .entry((row.event_id, row.field))
                .or_default()
                .push(row.value);
        }
    }
    for event in &mut events {
        if let Some(values) = fields.remove(&(event.id.clone(), "notifications".into())) {
            let numbers = values
                .iter()
                .map(|value| value.parse::<i64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("invalid Calendar notification offset: {error}"))?;
            event.notifications =
                Some(serde_json::to_string(&numbers).map_err(|error| error.to_string())?);
        }
        event.exceptions = fields
            .remove(&(event.id.clone(), "exceptions".into()))
            .map(|values| serde_json::to_string(&values))
            .transpose()
            .map_err(|error| error.to_string())?;
        event.rdate = fields
            .remove(&(event.id.clone(), "rdate".into()))
            .map(|values| serde_json::to_string(&values))
            .transpose()
            .map_err(|error| error.to_string())?;
    }
    Ok(WindowSource {
        events,
        overrides,
        attendees,
        total_event_count,
    })
}

impl WindowSource {
    /// Reuse canonical expansion for bounded prepared rows without staging SQL writes.
    pub(crate) fn from_prepared(
        events: Vec<DbCalendarEventRow>,
        overrides: Vec<DbOverrideRow>,
        attendees: Vec<DbWindowAttendeeRow>,
    ) -> Self {
        Self {
            events,
            overrides,
            attendees,
            total_event_count: None,
        }
    }

    pub fn expand(self, window: &Window) -> Result<NativeCalendarWindow, String> {
        let mut templates = Vec::new();
        let mut indices = Vec::new();
        let mut diagnostics = Vec::new();
        let mut grouped_overrides: BTreeMap<&str, Vec<&DbOverrideRow>> = BTreeMap::new();
        for row in &self.overrides {
            grouped_overrides
                .entry(&row.parent_event_id)
                .or_default()
                .push(row);
        }
        for (index, event) in self.events.iter().enumerate() {
            let exclusions = parse_list(event.exceptions.as_deref())?;
            let additions = parse_list(event.rdate.as_deref())?;
            let overrides = grouped_overrides
                .get(event.id.as_str())
                .into_iter()
                .flatten()
                .map(|row| StoredOverride {
                    recurrence_id: row.recurrence_id.clone(),
                    start: row.start_time.clone(),
                    end: row.end_time.clone(),
                    cancelled: row.status.as_deref() == Some("cancelled"),
                    this_and_future: row.recurrence_range.as_deref() == Some("this-and-future"),
                })
                .collect();
            match Template::from_stored(StoredTemplate {
                id: &event.id,
                start: &event.start_time,
                end: &event.end_time,
                home_zone: &event.timezone,
                all_day: event.all_day == 1,
                rrule: event.rrule.as_deref(),
                repeat_until: event.repeat_until.as_deref(),
                exceptions: &exclusions,
                rdates: &additions,
                overrides,
            }) {
                Ok(template) => {
                    templates.push(template);
                    indices.push(index);
                }
                Err(message) => diagnostics.push(ExpansionDiagnostic {
                    event_id: event.id.clone(),
                    message,
                }),
            }
        }
        let mut occurrences = Vec::new();
        for (index, expanded) in indices
            .into_iter()
            .zip(expand_templates(&templates, window)?)
        {
            let event = &self.events[index];
            let overrides = grouped_overrides.get(event.id.as_str());
            for occurrence in expanded {
                let override_id = occurrence
                    .override_index
                    .and_then(|index| overrides.and_then(|values| values.get(index)))
                    .map(|row| row.id.clone());
                occurrences.push(NativeOccurrence {
                    template_id: event.id.clone(),
                    id: occurrence.id,
                    recurring_parent_id: occurrence.parent_id,
                    recurrence_date: occurrence.recurrence_date.to_string(),
                    start_time: utc(occurrence.start_ms)?,
                    end_time: utc(occurrence.end_ms)?,
                    override_id,
                });
            }
        }
        occurrences.sort_by(|left, right| {
            left.start_time
                .cmp(&right.start_time)
                .then_with(|| left.id.cmp(&right.id))
        });
        let snapshot = NativeCalendarWindow {
            events: self.events,
            overrides: self.overrides,
            attendees: self.attendees,
            total_event_count: self.total_event_count,
            occurrences,
            diagnostics,
        };
        super::export::bound_snapshot_output(&snapshot)?;
        Ok(snapshot)
    }
}

fn parse_list(value: Option<&str>) -> Result<Vec<String>, String> {
    value
        .map(|value| {
            serde_json::from_str(value)
                .map_err(|error| format!("invalid canonical Calendar list: {error}"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests;
