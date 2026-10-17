//! Consistent, bounded Calendar export reads. The frontend retains the iCalendar codec.

use ganbaru_db::impl_sqlite_from_row;
use std::collections::BTreeMap;
use std::io::{self, Write};

use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, Row, SqliteConnection, SqlitePool, sqlite::SqliteRow};

use super::export_preservation::ExportPreservationRows;
use super::icalendar::calendar_icalendar_export_metadata;
use super::{
    CalendarFullEventRows, CalendarIcalendarExportMetadata, DbAlarmRow, DbAttendeeRow,
    DbFullEventRow, DbFullOverrideRow, FULL_EVENT_SELECT_SQL, sanitize_full_override_rows,
};
use crate::description::sanitize_calendar_description_html;
use crate::work_gate::WorkGate;

pub(super) const MAX_EXPORT_RECORDS: usize = 500_000;
pub(super) const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;
static EXPORT_GATE: WorkGate = WorkGate::new();

#[derive(Serialize)]
pub struct CalendarExportHeader {
    id: String,
    name: String,
    color: String,
    source: String,
    source_url: Option<String>,
}
impl_sqlite_from_row!(CalendarExportHeader {
    id,
    name,
    color,
    source,
    source_url
});

#[derive(Serialize)]
pub struct CalendarExportSnapshot {
    calendar: CalendarExportHeader,
    events: Vec<CalendarFullEventRows>,
    timezones: Vec<Value>,
    passthrough_components: Vec<Value>,
    metadata: CalendarIcalendarExportMetadata,
}

/// Read one authorized SQLite snapshot, then assemble bounded preservation off the async runtime.
pub(super) async fn load_export_snapshot(
    pool: &SqlitePool,
    calendar_id: &str,
) -> Result<CalendarExportSnapshot, String> {
    let permit = EXPORT_GATE.try_acquire().map_err(|_| {
        "A Calendar export is already being prepared. Try again when it finishes.".to_string()
    })?;
    let rows = read_export_rows(pool, calendar_id).await?;
    tokio::task::spawn_blocking(move || {
        // A cancelled IPC waiter must not release admission while this worker still runs.
        let _permit = permit;
        rows.assemble()
    })
    .await
    .map_err(|error| format!("assemble Calendar export worker: {error}"))?
}

/// Charge all table reads against one budget before allocating their rows.
pub(super) struct ExportBudget {
    records_left: usize,
    bytes_left: usize,
}

impl Default for ExportBudget {
    fn default() -> Self {
        Self {
            records_left: MAX_EXPORT_RECORDS,
            bytes_left: MAX_EXPORT_BYTES,
        }
    }
}

impl ExportBudget {
    pub(super) async fn read<T>(
        &mut self,
        connection: &mut SqliteConnection,
        calendar_id: &str,
        sql: &str,
        text_columns: &[&str],
    ) -> Result<Vec<T>, String>
    where
        for<'row> T: FromRow<'row, SqliteRow> + Send + Unpin,
    {
        // Query text and column names are repository constants, never caller input.
        // LIMIT bounds the preflight itself, including calendars with excessive row counts.
        let text_bytes = text_columns
            .iter()
            .map(|column| format!("COALESCE(length(CAST(\"{column}\" AS BLOB)), 0)"))
            .collect::<Vec<_>>()
            .join(" + ");
        let row_bytes = if text_bytes.is_empty() {
            "64".to_string()
        } else {
            format!("64 + {text_bytes}")
        };
        let preflight = format!(
            "SELECT COUNT(*) AS records, COALESCE(SUM({row_bytes}), 0) AS bytes FROM ({sql} LIMIT ?2)"
        );
        let limit = i64::try_from(self.records_left + 1)
            .map_err(|_| "Calendar export record limit overflow")?;
        let sizes = sqlx::query(&preflight)
            .bind(calendar_id)
            .bind(limit)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| format!("measure Calendar export rows: {error}"))?;
        let records = usize::try_from(
            sizes
                .try_get::<i64, _>("records")
                .map_err(|e| e.to_string())?,
        )
        .map_err(|_| "Invalid Calendar export record count")?;
        let bytes = usize::try_from(
            sizes
                .try_get::<i64, _>("bytes")
                .map_err(|e| e.to_string())?,
        )
        .map_err(|_| "Invalid Calendar export byte count")?;
        if records > self.records_left {
            return Err(format!(
                "Calendar export exceeds the {MAX_EXPORT_RECORDS} record limit"
            ));
        }
        if bytes > self.bytes_left {
            return Err(format!(
                "Calendar export exceeds the {MAX_EXPORT_BYTES} byte limit"
            ));
        }
        self.records_left -= records;
        self.bytes_left -= bytes;
        if records == 0 {
            return Ok(Vec::new());
        }
        sqlx::query_as(sql)
            .bind(calendar_id)
            .fetch_all(connection)
            .await
            .map_err(|error| format!("read Calendar export rows: {error}"))
    }
}

struct ExportField {
    owner_id: String,
    field: String,
    property_key: Option<String>,
    value: String,
}
impl_sqlite_from_row!(ExportField {
    owner_id,
    field,
    property_key,
    value
});

pub(super) struct CalendarExportRows {
    calendar: CalendarExportHeader,
    events: Vec<DbFullEventRow>,
    attendees: Vec<DbAttendeeRow>,
    alarms: Vec<DbAlarmRow>,
    overrides: Vec<DbFullOverrideRow>,
    fields: Vec<ExportField>,
    preservation: ExportPreservationRows,
}

async fn read_export_rows(
    pool: &SqlitePool,
    calendar_id: &str,
) -> Result<CalendarExportRows, String> {
    if calendar_id.trim().is_empty() || calendar_id.len() > 1024 {
        return Err("Invalid Calendar export identity".to_string());
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin Calendar export: {error}"))?;
    let rows = read_export_rows_on_connection(&mut transaction, calendar_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit Calendar export read: {error}"))?;
    Ok(rows)
}

/// Every query uses the caller's one read transaction, including metadata and preservation.
async fn read_export_rows_on_connection(
    connection: &mut SqliteConnection,
    calendar_id: &str,
) -> Result<CalendarExportRows, String> {
    let mut budget = ExportBudget::default();
    let calendar = budget
        .read::<CalendarExportHeader>(
            connection,
            calendar_id,
            "SELECT id, name, color, source, source_url FROM calendars WHERE id = ?1",
            &["id", "name", "color", "source", "source_url"],
        )
        .await?
        .pop()
        .ok_or_else(|| "Calendar no longer exists".to_string())?;
    let events = budget
        .read(
            connection,
            calendar_id,
            &format!(
                "{FULL_EVENT_SELECT_SQL} WHERE ce.calendar_id = ?1 ORDER BY ce.start_time, ce.id"
            ),
            &[
                "id",
                "title",
                "start_time",
                "end_time",
                "timezone",
                "calendar_id",
                "project_id",
                "environment_id",
                "playlist_id",
                "description",
                "rrule",
                "repeat_until",
                "location",
                "url",
                "transparency",
                "status",
                "source_uid",
                "visibility",
                "created_at",
                "local_rsvp_status",
                "icalendar_component_id",
                "icalendar_preservation_status",
                "rhythm_kind",
                "rhythm_source",
                "preset_key",
                "sequence_steps",
            ],
        )
        .await?;
    let attendees = budget
        .read(
            connection,
            calendar_id,
            "SELECT a.* FROM calendar_event_attendees a JOIN calendar_events e ON e.id = a.event_id
         WHERE e.calendar_id = ?1 ORDER BY a.event_id, a.sort_order, a.id",
            &[
                "id",
                "event_id",
                "icalendar_component_id",
                "name",
                "email",
                "role",
                "status",
            ],
        )
        .await?;
    let alarms = budget
        .read(
            connection,
            calendar_id,
            "SELECT a.* FROM calendar_event_alarms a JOIN calendar_events e ON e.id = a.event_id
         WHERE e.calendar_id = ?1 ORDER BY a.event_id, a.sort_order, a.id",
            &[
                "id",
                "event_id",
                "icalendar_component_id",
                "action",
                "trigger_type",
                "trigger_value",
                "description",
            ],
        )
        .await?;
    let overrides = budget
        .read(
            connection,
            calendar_id,
            "SELECT o.id, o.parent_event_id, o.recurrence_id, o.recurrence_range, o.title,
                o.start_time, o.end_time, o.description, o.location, o.url, o.color, o.status,
                o.transparency, o.visibility, NULL AS extended_properties,
                o.icalendar_component_id, NULL AS icalendar_raw_jcal
         FROM calendar_event_overrides o JOIN calendar_events e ON e.id = o.parent_event_id
         WHERE e.calendar_id = ?1 ORDER BY o.parent_event_id, o.recurrence_id, o.id",
            &[
                "id",
                "parent_event_id",
                "recurrence_id",
                "recurrence_range",
                "title",
                "start_time",
                "end_time",
                "description",
                "location",
                "url",
                "status",
                "transparency",
                "visibility",
                "icalendar_component_id",
            ],
        )
        .await?;
    let fields = read_projection_fields(connection, calendar_id, &mut budget).await?;
    let preservation = ExportPreservationRows::read(connection, calendar_id, &mut budget).await?;
    Ok(CalendarExportRows {
        calendar,
        events,
        attendees,
        alarms,
        overrides,
        fields,
        preservation,
    })
}

async fn read_projection_fields(
    connection: &mut SqliteConnection,
    calendar_id: &str,
    budget: &mut ExportBudget,
) -> Result<Vec<ExportField>, String> {
    let mut fields = Vec::new();
    for (table, field, value) in [
        (
            "calendar_event_notifications",
            "notifications",
            "offset_minutes",
        ),
        ("calendar_event_exdates", "exceptions", "occurrence_date"),
        ("calendar_event_rdates", "rdate", "occurrence_start"),
        ("calendar_event_categories", "categories", "category"),
    ] {
        let sql = format!(
            "SELECT c.event_id AS owner_id, '{field}' AS field, NULL AS property_key, CAST(c.{value} AS TEXT) AS value
             FROM {table} c JOIN calendar_events e ON e.id = c.event_id WHERE e.calendar_id = ?1
             ORDER BY c.event_id, c.sort_order, c.id"
        );
        fields.extend(
            budget
                .read(
                    connection,
                    calendar_id,
                    &sql,
                    &["owner_id", "field", "value"],
                )
                .await?,
        );
    }
    for (table, owner, join, field) in [
        (
            "calendar_event_extended_properties",
            "event_id",
            "JOIN calendar_events e ON e.id = c.event_id",
            "extended_properties",
        ),
        (
            "calendar_event_override_extended_properties",
            "override_id",
            "JOIN calendar_event_overrides o ON o.id = c.override_id JOIN calendar_events e ON e.id = o.parent_event_id",
            "override_properties",
        ),
    ] {
        let sql = format!(
            "SELECT c.{owner} AS owner_id, '{field}' AS field, c.property_key, c.property_value AS value
             FROM {table} c {join} WHERE e.calendar_id = ?1 ORDER BY c.{owner}, c.sort_order, c.id"
        );
        fields.extend(
            budget
                .read(
                    connection,
                    calendar_id,
                    &sql,
                    &["owner_id", "field", "property_key", "value"],
                )
                .await?,
        );
    }
    fields.extend(budget.read(connection, calendar_id,
        "SELECT e.id AS owner_id, 'organizer' AS field, NULL AS property_key, json_object('name', o.name, 'email', o.email) AS value
         FROM calendar_event_organizers o JOIN calendar_events e ON e.id = o.event_id WHERE e.calendar_id = ?1
         UNION ALL
         SELECT id, 'geo', NULL, json_object('lat', geo_lat, 'lng', geo_lng) FROM calendar_events
         WHERE calendar_id = ?1 AND geo_lat IS NOT NULL AND geo_lng IS NOT NULL",
        &["owner_id", "field", "value"],
    ).await?);
    Ok(fields)
}

impl CalendarExportRows {
    fn assemble(mut self) -> Result<CalendarExportSnapshot, String> {
        let preservation = self.preservation.assemble()?;
        let mut output_budget = ExportSizeWriter {
            remaining: MAX_EXPORT_BYTES,
        };
        charge_serialized(&mut output_budget, &self.calendar)?;
        let mut fields = assemble_fields(self.fields)?;
        let mut attendees = group_rows(self.attendees, |row| &row.event_id);
        let mut alarms = group_rows(self.alarms, |row| &row.event_id);
        for row in &mut self.overrides {
            row.extended_properties =
                fields.remove(&(row.id.clone(), "override_properties".to_string()));
            row.icalendar_raw_jcal =
                preservation.component_json(row.icalendar_component_id.as_deref())?;
        }
        sanitize_full_override_rows(&mut self.overrides);
        let mut overrides = group_rows(self.overrides, |row| &row.parent_event_id);
        let mut events = Vec::with_capacity(self.events.len());
        for mut event in self.events {
            event.description = sanitize_calendar_description_html(&event.description);
            for (field, target) in [
                ("notifications", &mut event.notifications),
                ("exceptions", &mut event.exceptions),
                ("rdate", &mut event.rdate),
                ("categories", &mut event.categories),
                ("extended_properties", &mut event.extended_properties),
                ("organizer", &mut event.organizer),
                ("geo", &mut event.geo),
            ] {
                *target = fields.remove(&(event.id.clone(), field.to_string()));
            }
            event.icalendar_raw_jcal =
                preservation.component_json(event.icalendar_component_id.as_deref())?;
            event.icalendar_projection_warnings =
                preservation.warnings_json(event.icalendar_component_id.as_deref())?;
            let rows = CalendarFullEventRows {
                attendees: attendees.remove(&event.id).unwrap_or_default(),
                alarms: alarms.remove(&event.id).unwrap_or_default(),
                overrides: overrides.remove(&event.id).unwrap_or_default(),
                event: Some(event),
            };
            charge_serialized(&mut output_budget, &rows)?;
            events.push(rows);
        }
        let timezones = preservation.timezones()?;
        charge_serialized(&mut output_budget, &timezones)?;
        let passthrough_components = preservation.passthrough()?;
        charge_serialized(&mut output_budget, &passthrough_components)?;
        let snapshot = CalendarExportSnapshot {
            calendar: self.calendar,
            events,
            timezones,
            passthrough_components,
            metadata: calendar_icalendar_export_metadata(preservation.methods),
        };
        // JSON escaping and repeated preserved subtrees can exceed the source byte budget.
        serde_json::to_writer(
            ExportSizeWriter {
                remaining: MAX_EXPORT_BYTES,
            },
            &snapshot,
        )
        .map_err(|error| {
            format!("Calendar export snapshot exceeds its byte limit or cannot serialize: {error}")
        })?;
        Ok(snapshot)
    }
}

fn charge_serialized(writer: &mut ExportSizeWriter, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(writer, value).map_err(|error| {
        format!("Calendar export snapshot exceeds its byte limit or cannot serialize: {error}")
    })
}

/// Bound escaped output bytes without allocating a second serialized snapshot.
pub(super) fn bound_snapshot_output(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(
        ExportSizeWriter {
            remaining: MAX_EXPORT_BYTES,
        },
        value,
    )
    .map_err(|error| {
        format!("Calendar snapshot exceeds its byte limit or cannot serialize: {error}")
    })
}

fn group_rows<T>(rows: Vec<T>, owner: impl Fn(&T) -> &str) -> BTreeMap<String, Vec<T>> {
    let mut grouped: BTreeMap<String, Vec<T>> = BTreeMap::new();
    for row in rows {
        grouped
            .entry(owner(&row).to_string())
            .or_default()
            .push(row);
    }
    grouped
}

fn assemble_fields(rows: Vec<ExportField>) -> Result<BTreeMap<(String, String), String>, String> {
    let mut fields: BTreeMap<(String, String), Value> = BTreeMap::new();
    for row in rows {
        let key = (row.owner_id, row.field.clone());
        if let Some(property_key) = row.property_key {
            let value = fields
                .entry(key)
                .or_insert_with(|| Value::Object(Default::default()));
            value
                .as_object_mut()
                .ok_or("Invalid Calendar export property list")?
                .insert(property_key, Value::String(row.value));
        } else if matches!(row.field.as_str(), "organizer" | "geo") {
            fields.insert(
                key,
                serde_json::from_str(&row.value).map_err(|error| error.to_string())?,
            );
        } else {
            let value = if row.field == "notifications" {
                Value::Number(
                    row.value
                        .parse::<i64>()
                        .map_err(|error| error.to_string())?
                        .into(),
                )
            } else {
                Value::String(row.value)
            };
            fields
                .entry(key)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or("Invalid Calendar export value list")?
                .push(value);
        }
    }
    fields
        .into_iter()
        .map(|(key, value)| {
            serde_json::to_string(&value)
                .map(|json| (key, json))
                .map_err(|error| error.to_string())
        })
        .collect()
}

struct ExportSizeWriter {
    remaining: usize,
}

impl Write for ExportSizeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("Calendar export byte limit exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
