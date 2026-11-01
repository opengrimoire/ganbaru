//! One consistent native Calendar read and home-zone recurrence projection.

use ganbaru_db::impl_sqlite_from_row;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, FromRow, SqliteConnection, SqlitePool};

use super::event_music::{EVENT_MUSIC_SQL, EventMusicAssignment, event_music_flags};
use super::export::MAX_EXPORT_BYTES;
use super::{DbCalendarEventRow, DbOverrideRow, DbWindowAttendeeRow};
use crate::recurrence::canonical::{
    Occurrence, StoredOverride, StoredTemplate, Template, Window, expand_each_template,
};
use crate::recurrence::rule::MAX_RULE_BYTES;
use crate::work_gate::WorkGate;
use ganbaru_civil_time as civil_time;

const MAX_TEMPLATES: usize = 10_000;
/// The frontend window parser rejects a payload above this many occurrences.
const MAX_WINDOW_OCCURRENCES: usize = 10_000;
const MAX_ID_BYTES: usize = 1_024;
/// Code points of title and location a window carries; the full text loads with the event.
const WINDOW_TEXT_CHARS: usize = 1_024;
/// Longest timestamp, zone, or repeat-until value a window admits.
const WINDOW_STRUCTURAL_CHARS: usize = 128;
/// Largest integer the frontend window parser accepts.
const WINDOW_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const WINDOW_TEXT_BUDGET_BYTES: usize = 16 * 1024 * 1024;
const WINDOW_RECORD_BUDGET: usize = 100_000;
/// A template above this share of a budget is admitted only after every lighter template.
const HEAVY_TEMPLATE_DIVISOR: usize = 16;
const ROW_OVERHEAD_BYTES: usize = 64;
const SEQUENCE_STEP_BYTES: usize = 128;
const NOTIFICATION_ROW_BYTES: usize = 24;
const LIST_VALUE_OVERHEAD_BYTES: usize = 3;
/// The frontend window parser charges three bytes for each UTF-16 unit of text.
const FRONTEND_BYTES_PER_TEXT_UNIT: usize = 3;
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
    /// Selected templates left out to keep the window inside its bounds.
    pub omitted_template_count: usize,
}

impl NativeCalendarWindow {
    /// Removes templates with their children, occurrences, and diagnostics, so that every
    /// remaining row keeps a selected owner.
    fn remove_templates(&mut self, removed: &BTreeSet<String>) {
        self.events.retain(|row| !removed.contains(&row.id));
        self.overrides
            .retain(|row| !removed.contains(&row.parent_event_id));
        self.attendees
            .retain(|row| !removed.contains(&row.event_id));
        self.occurrences
            .retain(|row| !removed.contains(&row.template_id));
        self.diagnostics
            .retain(|row| !removed.contains(&row.event_id));
        self.omitted_template_count += removed.len();
    }

    /// Keeps the first occurrence of each identity, since the frontend rejects a repeated one, and
    /// reports the template that lost its occurrence.
    fn remove_repeated_occurrences(&mut self) {
        let mut seen = HashSet::new();
        let mut repeated = Vec::new();
        self.occurrences.retain(|row| {
            let unique = seen.insert(row.id.clone());
            if !unique {
                repeated.push(ExpansionDiagnostic {
                    event_id: row.template_id.clone(),
                    message: format!("Occurrence identity {} is already in use", row.id),
                });
            }
            unique
        });
        self.diagnostics.append(&mut repeated);
    }

    /// Whether both the serialized payload and the frontend text charge fit their limit.
    fn fits_limits(&self) -> bool {
        let Ok(bytes) = super::export::bound_snapshot_output(self) else {
            return false;
        };
        // Serialized text is never shorter than its UTF-16 form, so most windows stop here.
        if bytes.saturating_mul(FRONTEND_BYTES_PER_TEXT_UNIT) <= MAX_EXPORT_BYTES {
            return true;
        }
        serde_json::to_value(self).is_ok_and(|value| {
            text_units(&value).saturating_mul(FRONTEND_BYTES_PER_TEXT_UNIT) <= MAX_EXPORT_BYTES
        })
    }
}

/// UTF-16 units across every string value of a serialized window.
fn text_units(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(text) => text.encode_utf16().count(),
        serde_json::Value::Array(values) => values.iter().map(text_units).sum(),
        serde_json::Value::Object(fields) => fields.values().map(text_units).sum(),
        _ => 0,
    }
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

/// The window columns of a template, except its shortened title and location.
const EVENT_COLUMNS_SQL: &str = r#"
           ce.id, ce.start_time, ce.end_time, ce.timezone,
           ce.calendar_id, ce.project_id, ce.environment_id, ce.playlist_id, ce.color, ce.rrule,
           NULL AS notifications, NULL AS exceptions, ce.repeat_until,
           ce.all_day, ce.transparency, ce.status,
           CASE WHEN ce.url <> '' THEN 1 ELSE 0 END AS has_call_link,
           ce.meeting_enabled, 0 AS has_music_playlist, 0 AS has_soundscape,
           ce.local_rsvp_status, ce.created_at,
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
"#;

/// Every template a window selects. `?1` is the serialized selection.
const SELECTED_SOURCE_SQL: &str = r#"
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
"#;

/// The SQL sum of the stored bytes of each column.
fn stored_bytes_sql(columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("COALESCE(length(CAST({column} AS BLOB)), 0)"))
        .collect::<Vec<_>>()
        .join(" + ")
}

/// A window column truncated to the text a window carries.
fn window_text_sql(column: &str) -> String {
    format!("substr({column}, 1, {WINDOW_TEXT_CHARS})")
}

/// Price every selected template with what it owns, then admit a prefix of them, lightest tier
/// first, so that a flood of templates cannot displace honest ones or fail the read. A template
/// the frontend cannot represent is never admitted, and neither are the ones past a budget.
fn admission_sql() -> String {
    let id_bytes = stored_bytes_sql(&["ce.id"]);
    let rule_bytes = stored_bytes_sql(&["ce.rrule"]);
    let override_id_bytes = stored_bytes_sql(&["o.id"]);
    let event_bytes = stored_bytes_sql(&[
        "ce.id",
        &window_text_sql("ce.title"),
        &window_text_sql("ce.location"),
        "ce.start_time",
        "ce.end_time",
        "ce.timezone",
        "ce.calendar_id",
        "ce.project_id",
        "ce.environment_id",
        "ce.playlist_id",
        "ce.rrule",
        "ce.repeat_until",
        "ce.transparency",
        "ce.status",
        "ce.local_rsvp_status",
        "ce.created_at",
        "pc.rhythm_kind",
        "pc.rhythm_source",
        "pc.preset_key",
    ]);
    let override_bytes = stored_bytes_sql(&[
        "o.id",
        "o.parent_event_id",
        "o.recurrence_id",
        "o.recurrence_range",
        "o.title",
        "o.start_time",
        "o.end_time",
        "o.status",
        "o.transparency",
    ]);
    let sequence_steps = "(SELECT COUNT(*) FROM calendar_event_pomodoro_config_sequence_steps pcss
                           WHERE pcss.event_id = ce.id)";
    let attendee_bytes = stored_bytes_sql(&["a.event_id", "a.email", "a.status"]);
    let exdate_bytes = stored_bytes_sql(&["x.occurrence_date"]);
    let rdate_bytes = stored_bytes_sql(&["r.occurrence_start"]);
    let heavy_bytes = WINDOW_TEXT_BUDGET_BYTES / HEAVY_TEMPLATE_DIVISOR;
    let heavy_records = WINDOW_RECORD_BUDGET / HEAVY_TEMPLATE_DIVISOR;
    format!(
        r#"
    WITH selected AS MATERIALIZED (
      SELECT ce.id, ce.start_time,
        (COALESCE(ce.rrule, '') = ''
          AND NOT EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
          AND NOT EXISTS (SELECT 1 FROM calendar_event_overrides o WHERE o.parent_event_id = ce.id)
        ) AS simple,
        ({id_bytes} > {MAX_ID_BYTES}
          OR {rule_bytes} > {MAX_RULE_BYTES}
          OR length(ce.timezone) > {WINDOW_STRUCTURAL_CHARS}
          OR length(ce.start_time) > {WINDOW_STRUCTURAL_CHARS}
          OR length(ce.end_time) > {WINDOW_STRUCTURAL_CHARS}
          OR COALESCE(length(ce.repeat_until), 0) > {WINDOW_STRUCTURAL_CHARS}
          OR COALESCE(pc.idle_timeout_minutes, 0) > {WINDOW_MAX_SAFE_INTEGER}
          OR COALESCE(pcc.focus_duration_minutes, 0) > {WINDOW_MAX_SAFE_INTEGER}
          OR COALESCE(pcc.short_break_minutes, 0) > {WINDOW_MAX_SAFE_INTEGER}
          OR COALESCE(pcc.long_break_minutes, 0) > {WINDOW_MAX_SAFE_INTEGER}
          OR COALESCE(pcc.long_break_after_focus_count, 0) > {WINDOW_MAX_SAFE_INTEGER}
          OR EXISTS (SELECT 1 FROM calendar_event_overrides o
                     WHERE o.parent_event_id = ce.id AND {override_id_bytes} > {MAX_ID_BYTES})
        ) AS rejected,
        1 + {sequence_steps} AS records,
        {ROW_OVERHEAD_BYTES} + {event_bytes} + {sequence_steps} * {SEQUENCE_STEP_BYTES} AS bytes
      {SELECTED_SOURCE_SQL}
    ),
    owned(event_id, records, bytes) AS (
      SELECT o.parent_event_id, 1, {ROW_OVERHEAD_BYTES} + {override_bytes}
        FROM calendar_event_overrides o JOIN selected s ON s.id = o.parent_event_id
      UNION ALL
      SELECT a.event_id, 1, {ROW_OVERHEAD_BYTES} + {attendee_bytes}
        FROM calendar_event_attendees a JOIN selected s ON s.id = a.event_id
        WHERE json_extract(?1, '$.purpose') = 'render'
      UNION ALL
      SELECT n.event_id, 1, {NOTIFICATION_ROW_BYTES}
        FROM calendar_event_notifications n JOIN selected s ON s.id = n.event_id
      UNION ALL
      SELECT x.event_id, 1, {LIST_VALUE_OVERHEAD_BYTES} + {exdate_bytes}
        FROM calendar_event_exdates x JOIN selected s ON s.id = x.event_id
      UNION ALL
      SELECT r.event_id, 1, {LIST_VALUE_OVERHEAD_BYTES} + {rdate_bytes}
        FROM calendar_event_rdates r JOIN selected s ON s.id = r.event_id
    ),
    totals AS (
      SELECT event_id, SUM(records) AS records, SUM(bytes) AS bytes FROM owned GROUP BY event_id
    ),
    priced AS (
      SELECT s.id, s.start_time, s.simple,
             s.records + COALESCE(t.records, 0) AS records,
             s.bytes + COALESCE(t.bytes, 0) AS bytes
      FROM selected s LEFT JOIN totals t ON t.event_id = s.id
      WHERE NOT s.rejected
    ),
    tiered AS (
      SELECT id, start_time, records, bytes,
             CASE WHEN bytes > {heavy_bytes} OR records > {heavy_records} THEN 2
                  WHEN simple THEN 0 ELSE 1 END AS tier
      FROM priced
    ),
    running AS (
      SELECT id, ROW_NUMBER() OVER prefix AS position,
             SUM(bytes) OVER prefix AS total_bytes,
             SUM(records) OVER prefix AS total_records
      FROM tiered
      WINDOW prefix AS (ORDER BY tier, start_time, id)
    ),
    admitted AS (
      SELECT id, position FROM running
      WHERE position <= {MAX_TEMPLATES}
        AND total_bytes <= {WINDOW_TEXT_BUDGET_BYTES}
        AND total_records <= {WINDOW_RECORD_BUDGET}
    )
    SELECT (SELECT json_group_array(id ORDER BY position) FROM admitted) AS ids,
           (SELECT COUNT(*) FROM selected) - (SELECT COUNT(*) FROM admitted) AS omitted
    "#
    )
}

fn admitted_events_sql() -> String {
    format!(
        "SELECT {title} AS title, {location} AS location, {EVENT_COLUMNS_SQL}
         FROM json_each(?1) admitted
         JOIN calendar_events ce ON ce.id = admitted.value
         LEFT JOIN calendar_event_pomodoro_configs pc ON pc.event_id = ce.id
         LEFT JOIN calendar_event_pomodoro_config_count_rhythms pcc ON pcc.event_id = ce.id
         ORDER BY admitted.key",
        title = window_text_sql("ce.title"),
        location = window_text_sql("ce.location"),
    )
}

/// Reads the rows of the admitted templates. Admission has already priced them.
async fn read_rows<T>(
    connection: &mut SqliteConnection,
    ids: &str,
    sql: &str,
) -> Result<Vec<T>, String>
where
    for<'row> T: FromRow<'row, SqliteRow> + Send + Unpin,
{
    sqlx::query_as(AssertSqlSafe(sql))
        .bind(ids)
        .fetch_all(&mut *connection)
        .await
        .map_err(|error| format!("read native Calendar window rows: {error}"))
}

struct WindowAdmission {
    ids: String,
    omitted: i64,
}
impl_sqlite_from_row!(WindowAdmission { ids, omitted });

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
    omitted_template_count: usize,
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
    let render = matches!(purpose, WindowPurpose::Render);
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
    let admission: WindowAdmission = sqlx::query_as(AssertSqlSafe(admission_sql()))
        .bind(&selection)
        .fetch_one(&mut *connection)
        .await
        .map_err(|error| format!("admit native Calendar templates: {error}"))?;
    let omitted_template_count = usize::try_from(admission.omitted)
        .map_err(|_| "Invalid omitted Calendar template count")?;
    let ids = admission.ids;
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
    if ids == "[]" {
        return Ok(WindowSource {
            events: Vec::new(),
            overrides: Vec::new(),
            attendees: Vec::new(),
            total_event_count,
            omitted_template_count,
        });
    }
    let mut events: Vec<DbCalendarEventRow> =
        read_rows(connection, &ids, &admitted_events_sql()).await?;
    let overrides: Vec<DbOverrideRow> = read_rows(connection, &ids,
        "SELECT id, parent_event_id, recurrence_id, recurrence_range, title, start_time, end_time, color, status, transparency
         FROM calendar_event_overrides WHERE parent_event_id IN (SELECT value FROM json_each(?1))
         ORDER BY parent_event_id, recurrence_id, id",
    ).await?;
    let attendees = if !render {
        Vec::new()
    } else {
        read_rows(
            connection,
            &ids,
            "SELECT event_id, email, status FROM calendar_event_attendees
             WHERE event_id IN (SELECT value FROM json_each(?1)) ORDER BY event_id, sort_order, id",
        )
        .await?
    };
    if render {
        let assignments: Vec<EventMusicAssignment> =
            read_rows(connection, &ids, EVENT_MUSIC_SQL).await?;
        let mut by_event: BTreeMap<&str, Vec<&EventMusicAssignment>> = BTreeMap::new();
        for row in &assignments {
            by_event.entry(&row.owner_id).or_default().push(row);
        }
        for event in &mut events {
            let Some(rows) = by_event.get(event.id.as_str()) else {
                continue;
            };
            let flags = event_music_flags(rows.iter().copied(), event.rhythm_kind.is_some());
            event.has_music_playlist = i64::from(flags.playlist);
            event.has_soundscape = i64::from(flags.soundscape);
        }
    }
    let mut fields: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (table, field, column, order) in [
        (
            "calendar_event_notifications",
            "notifications",
            "offset_minutes",
            "sort_order",
        ),
        (
            "calendar_event_exdates",
            "exceptions",
            "occurrence_date",
            "occurrence_date",
        ),
        (
            "calendar_event_rdates",
            "rdate",
            "occurrence_start",
            "occurrence_start",
        ),
    ] {
        let rows: Vec<ProjectionField> = read_rows(
            connection,
            &ids,
            &format!(
                "SELECT event_id, '{field}' AS field, CAST({column} AS TEXT) AS value FROM {table}
             WHERE event_id IN (SELECT value FROM json_each(?1)) ORDER BY event_id, {order}, id"
            ),
        )
        .await?;
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
        omitted_template_count,
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
            omitted_template_count: 0,
        }
    }

    /// Expands the admitted templates, omitting whichever ones would break a window bound.
    /// Templates go in admission order, so the last admitted are the first omitted.
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
            match template_for(event, grouped_overrides.get(event.id.as_str())) {
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
        let mut over_occurrence_limit = BTreeSet::new();
        for (index, expanded) in indices
            .into_iter()
            .zip(expand_each_template(&templates, window))
        {
            let event = &self.events[index];
            let overrides = grouped_overrides.get(event.id.as_str());
            match expanded.and_then(|expanded| native_occurrences(event, overrides, expanded)) {
                Ok(values) if occurrences.len() + values.len() > MAX_WINDOW_OCCURRENCES => {
                    over_occurrence_limit.insert(event.id.clone());
                }
                Ok(values) => occurrences.extend(values),
                Err(message) => diagnostics.push(ExpansionDiagnostic {
                    event_id: event.id.clone(),
                    message,
                }),
            }
        }
        occurrences.sort_by(|left, right| {
            left.start_time
                .cmp(&right.start_time)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut snapshot = NativeCalendarWindow {
            events: self.events,
            overrides: self.overrides,
            attendees: self.attendees,
            total_event_count: self.total_event_count,
            occurrences,
            diagnostics,
            omitted_template_count: self.omitted_template_count,
        };
        snapshot.remove_templates(&over_occurrence_limit);
        snapshot.remove_repeated_occurrences();
        // Escaped text can outgrow what admission charged for it. Each pass removes a quarter of
        // the templates, so the number of serializations stays logarithmic.
        while !snapshot.events.is_empty() && !snapshot.fits_limits() {
            let kept = snapshot.events.len() - snapshot.events.len().div_ceil(4);
            let removed = snapshot.events[kept..]
                .iter()
                .map(|event| event.id.clone())
                .collect();
            snapshot.remove_templates(&removed);
        }
        super::export::bound_snapshot_output(&snapshot)?;
        snapshot.events.sort_by(|left, right| {
            (&left.start_time, &left.id).cmp(&(&right.start_time, &right.id))
        });
        Ok(snapshot)
    }
}

/// Decodes one stored event and its overrides into a canonical template.
fn template_for(
    event: &DbCalendarEventRow,
    overrides: Option<&Vec<&DbOverrideRow>>,
) -> Result<Template, String> {
    let exclusions = parse_list(event.exceptions.as_deref())?;
    let additions = parse_list(event.rdate.as_deref())?;
    let overrides = overrides
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
    Template::from_stored(StoredTemplate {
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
    })
}

/// Converts one template's expansion into window occurrences linked to their override rows.
fn native_occurrences(
    event: &DbCalendarEventRow,
    overrides: Option<&Vec<&DbOverrideRow>>,
    expanded: Vec<Occurrence>,
) -> Result<Vec<NativeOccurrence>, String> {
    expanded
        .into_iter()
        .map(|occurrence| {
            let override_id = occurrence
                .override_index
                .and_then(|index| overrides.and_then(|values| values.get(index)))
                .map(|row| row.id.clone());
            Ok(NativeOccurrence {
                template_id: event.id.clone(),
                id: occurrence.id,
                recurring_parent_id: occurrence.parent_id,
                recurrence_date: occurrence.recurrence_date.to_string(),
                start_time: utc(occurrence.start_ms)?,
                end_time: utc(occurrence.end_ms)?,
                override_id,
            })
        })
        .collect()
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
