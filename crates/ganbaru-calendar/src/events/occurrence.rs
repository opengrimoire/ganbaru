//! Canonical mutation geometry read from the caller's transaction.

use ganbaru_db::impl_sqlite_from_row;

use sqlx::{FromRow, SqliteConnection, sqlite::SqliteRow};

use crate::reads::native_window::utc;
use crate::recurrence::canonical::{StoredOverride, StoredTemplate, Template, parse_date};
use crate::work_gate::WorkGate;

use super::ids::split_synthetic_id;
use super::types::CalendarEventMutationContext;

const MAX_SOURCE_RECORDS: usize = 10_000;
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ID_BYTES: usize = 1_024;
static LOOKUP_GATE: WorkGate = WorkGate::new();

pub(super) fn validate_source_id(source_id: &str) -> Result<(), String> {
    if source_id.trim().is_empty()
        || source_id.len() > MAX_ID_BYTES
        || source_id.contains("::")
        || source_id.chars().any(char::is_control)
    {
        return Err("Calendar source identity must be a bounded canonical event ID".into());
    }
    Ok(())
}

pub(super) struct Source {
    pub id: String,
    pub start_time: String,
    pub end_time: String,
    pub timezone: String,
    pub all_day: i64,
    pub rrule: Option<String>,
    pub repeat_until: Option<String>,
}
impl_sqlite_from_row!(Source {
    id,
    start_time,
    end_time,
    timezone,
    all_day,
    rrule,
    repeat_until
});

struct Override {
    recurrence_id: String,
    start_time: Option<String>,
    end_time: Option<String>,
    status: Option<String>,
    recurrence_range: Option<String>,
}
impl_sqlite_from_row!(Override {
    recurrence_id,
    start_time,
    end_time,
    status,
    recurrence_range
});

/// One record and byte allowance covers all geometry reads, before allocation.
pub(crate) struct ReadBudget {
    records: usize,
    bytes: usize,
}

impl Default for ReadBudget {
    fn default() -> Self {
        Self {
            records: MAX_SOURCE_RECORDS,
            bytes: MAX_SOURCE_BYTES,
        }
    }
}

impl ReadBudget {
    pub(crate) async fn read<T>(
        &mut self,
        connection: &mut SqliteConnection,
        id: &str,
        sql: &str,
        columns: &[&str],
    ) -> Result<Vec<T>, String>
    where
        for<'row> T: FromRow<'row, SqliteRow> + Send + Unpin,
    {
        // SQL and column names are repository constants, never request values.
        let bytes = columns
            .iter()
            .map(|column| format!("COALESCE(length(CAST({column} AS BLOB)), 0)"))
            .collect::<Vec<_>>()
            .join(" + ");
        let limited = format!("{sql} LIMIT ?2");
        let preflight = format!("SELECT COUNT(*), COALESCE(SUM(64 + {bytes}), 0) FROM ({limited})");
        let limit = i64::try_from(self.records + 1)
            .map_err(|_| "Calendar occurrence read limit overflow")?;
        let (records, bytes): (i64, i64) = sqlx::query_as(&preflight)
            .bind(id)
            .bind(limit)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| format!("measure Calendar occurrence source: {error}"))?;
        let records = usize::try_from(records).map_err(|_| "invalid occurrence record count")?;
        let bytes = usize::try_from(bytes).map_err(|_| "invalid occurrence byte count")?;
        if records > self.records || bytes > self.bytes {
            return Err("Calendar occurrence source exceeds its record or byte budget".into());
        }
        self.records -= records;
        self.bytes -= bytes;
        sqlx::query_as(&limited)
            .bind(id)
            .bind(limit)
            .fetch_all(connection)
            .await
            .map_err(|error| format!("read Calendar occurrence source: {error}"))
    }
}

/// Persisted geometry captured in a caller-owned snapshot, before CPU planning.
pub(super) struct Geometry {
    pub source: Source,
    exceptions: Vec<String>,
    rdates: Vec<String>,
    overrides: Vec<Override>,
}

impl Geometry {
    /// Decode one complete native recurrence source after bounded SQL admission.
    pub fn template(&self, include_recurrence: bool) -> Result<Template, String> {
        Template::from_stored(StoredTemplate {
            id: &self.source.id,
            start: &self.source.start_time,
            end: &self.source.end_time,
            home_zone: &self.source.timezone,
            all_day: self.source.all_day != 0,
            rrule: include_recurrence
                .then_some(self.source.rrule.as_deref())
                .flatten(),
            repeat_until: include_recurrence
                .then_some(self.source.repeat_until.as_deref())
                .flatten(),
            exceptions: &self.exceptions,
            rdates: &self.rdates,
            overrides: self
                .overrides
                .iter()
                .map(|value| StoredOverride {
                    recurrence_id: value.recurrence_id.clone(),
                    start: value.start_time.clone(),
                    end: value.end_time.clone(),
                    cancelled: value.status.as_deref() == Some("cancelled"),
                    this_and_future: value.recurrence_range.as_deref() == Some("this-and-future"),
                })
                .collect(),
        })
    }
}

/// Share source allocation limits with other rows needed by the semantic operation.
pub(super) async fn read_geometry(
    connection: &mut SqliteConnection,
    source_id: &str,
    include_recurrence: bool,
    budget: &mut ReadBudget,
) -> Result<Geometry, String> {
    validate_source_id(source_id)?;
    let source = budget.read::<Source>(connection, source_id,
        "SELECT id, start_time, end_time, timezone, all_day, rrule, repeat_until FROM calendar_events WHERE id = ?1",
        &["id", "start_time", "end_time", "timezone", "rrule", "repeat_until"]
    ).await?.pop().ok_or_else(|| format!("calendar event '{source_id}' not found"))?;
    let mut geometry = Geometry {
        source,
        exceptions: Vec::new(),
        rdates: Vec::new(),
        overrides: Vec::new(),
    };
    if include_recurrence {
        geometry.exceptions = budget
            .read::<(String,)>(
                connection,
                source_id,
                "SELECT occurrence_date FROM calendar_event_exdates WHERE event_id = ?1",
                &["occurrence_date"],
            )
            .await?
            .into_iter()
            .map(|row| row.0)
            .collect();
        geometry.rdates = budget
            .read::<(String,)>(
                connection,
                source_id,
                "SELECT occurrence_start FROM calendar_event_rdates WHERE event_id = ?1",
                &["occurrence_start"],
            )
            .await?
            .into_iter()
            .map(|row| row.0)
            .collect();
        geometry.overrides = budget.read::<Override>(connection, source_id,
            "SELECT recurrence_id, start_time, end_time, status, recurrence_range FROM calendar_event_overrides WHERE parent_event_id = ?1",
            &["recurrence_id", "start_time", "end_time", "status", "recurrence_range"]
        ).await?;
    }
    Ok(geometry)
}

/// Resolve trusted geometry without accepting frontend start/end authority.
/// SQL stays in the mutation transaction; bounded CPU work is awaited off-thread.
pub(super) async fn load_context(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<CalendarEventMutationContext, String> {
    let permit = LOOKUP_GATE
        .try_acquire()
        .map_err(|_| "A Calendar occurrence is being resolved; retry after it finishes")?;
    let (source_id, date) = split_synthetic_id(id);
    if source_id.trim().is_empty() || source_id.len() > MAX_ID_BYTES {
        return Err("Calendar source identity is empty or exceeds 1024 bytes".into());
    }
    if date.is_some_and(|value| value.len() != 10) {
        return Err("Calendar occurrence identity requires a YYYY-MM-DD date".into());
    }
    let date = date.map(parse_date).transpose()?;
    let geometry = read_geometry(
        connection,
        source_id,
        date.is_some(),
        &mut ReadBudget::default(),
    )
    .await?;
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        // Cancellation of the waiter cannot admit a second CPU lookup prematurely.
        let _permit = permit;
        // Root mutations remain available for preservation-only recurrence rules.
        let template = geometry.template(date.is_some())?;
        let occurrence = template
            .resolve_identity(date)?
            .ok_or_else(|| format!("Calendar occurrence '{id}' no longer exists"))?;
        Ok(CalendarEventMutationContext {
            id,
            canonical_id: occurrence.id,
            source_event_id: geometry.source.id,
            occurrence_date: date.map(|value| value.to_string()),
            start_time: utc(occurrence.start_ms)?,
            end_time: utc(occurrence.end_ms)?,
            rrule: geometry.source.rrule,
            repeat_until: geometry.source.repeat_until,
            synthetic: date.is_some(),
        })
    })
    .await
    .map_err(|error| format!("Calendar occurrence lookup worker: {error}"))?
}
