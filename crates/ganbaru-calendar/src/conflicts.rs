//! Conflicts on event titles, descriptions, and timing.
//!
//! Concurrent edits from linked devices whose values differ leave one displayed value on every
//! replica and the others as versions to choose from. Resolving writes the chosen value as a
//! local edit and seals the field, so the conflict closes on every device. A timing version is
//! written whole: the start, end, zone, all-day flag, and recurrence columns together with the
//! event's excluded dates, added dates, and overrides, which keep the ids the version carries so
//! two devices choosing the same version write the same value.
//!
//! A resolution changes event rows outside the native Focus edit path, as rows sync applies do,
//! so callers refresh Calendar views and the Focus projection afterwards the same way.

use ganbaru_sync::domains::calendar::decode_rows;
use ganbaru_sync::manifest::OwnedTable;
use ganbaru_sync::manifest::vault::calendar::{
    EVENTS, EVENTS_TABLE, EXDATES, OVERRIDE_PROPERTIES, OVERRIDES, RDATES, event_group,
};
use ganbaru_sync::{ConflictDetail, ConflictVersion, DeviceNames, DeviceRef, Engine, local};
use ganbaru_sync_contracts::{Field, GroupId, SpaceId, Value, WriterId};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Sqlite, SqliteConnection, SqlitePool, Transaction};

use crate::recovery::{
    COMPONENT_ID, RetainedRow, UPDATED_AT, bind_field, insert, live_reference, now,
};

/// Stable conflict outcomes, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum CalendarConflictError {
    /// The conflict or the chosen version no longer exists; reload and show the current state.
    Resolved(String),
    Failed(String),
}

impl From<String> for CalendarConflictError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// An event field that can hold a conflict.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalendarConflictField {
    Title,
    Description,
    Timing,
}

impl CalendarConflictField {
    const ALL: [Self; 3] = [Self::Title, Self::Description, Self::Timing];

    fn group(self) -> GroupId {
        match self {
            Self::Title => event_group::TITLE,
            Self::Description => event_group::DESCRIPTION,
            Self::Timing => event_group::TIMING,
        }
    }

    fn from_group(group: GroupId) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.group() == group)
    }
}

/// The timing of one version, as the dialog summarizes it.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarTimingSummary {
    start_time: String,
    end_time: String,
    timezone: String,
    all_day: bool,
    /// The recurrence rule, absent for a single event.
    rrule: Option<String>,
    repeat_until: Option<String>,
    excluded_dates: usize,
    added_dates: usize,
    overrides: usize,
}

/// The value of one version.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalendarConflictValue {
    Text { text: String },
    Timing(CalendarTimingSummary),
}

/// One concurrent version of a field.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarConflictVersion {
    /// Opaque version id for resolving.
    version: String,
    device: DeviceRef,
    /// When the version was written, in Unix milliseconds.
    edited_at_ms: u64,
    /// Whether this version is the one every device shows.
    displayed: bool,
    value: CalendarConflictValue,
}

/// The versions of one event field in conflict, newest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarFieldConflict {
    event_id: String,
    field: CalendarConflictField,
    versions: Vec<CalendarConflictVersion>,
}

/// The open conflicts of every event; empty when there are none.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarConflictRead {
    conflicts: Vec<CalendarFieldConflict>,
}

/// How to resolve a field in conflict.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalendarConflictChoice {
    /// Keep the displayed version.
    Displayed,
    /// Use another version.
    Version { version: String },
}

/// A conflict resolution request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarConflictResolution {
    pub event_id: String,
    pub field: CalendarConflictField,
    pub choice: CalendarConflictChoice,
}

/// Text of a single-text group value.
fn text_from_value(value: &Value) -> Result<String, String> {
    match value.fields() {
        [Field::Text(text)] => Ok(text.clone()),
        _ => Err("calendar text value is invalid".to_string()),
    }
}

fn column_text(row: &RetainedRow, column: &str) -> Result<Option<String>, String> {
    match row.columns.iter().find(|(name, _)| *name == column) {
        Some((_, Field::Text(text))) => Ok(Some(text.clone())),
        Some((_, Field::Null)) => Ok(None),
        _ => Err(format!("calendar timing value has no {column}")),
    }
}

fn required_text(row: &RetainedRow, column: &str) -> Result<String, String> {
    column_text(row, column)?.ok_or_else(|| format!("calendar timing value has no {column}"))
}

/// The number of owned rows of `table` in a timing value.
fn owned_count(row: &RetainedRow, table: &OwnedTable) -> Result<usize, String> {
    let Some((owned, bytes)) = row
        .owned
        .iter()
        .find(|(owned, _)| owned.table == table.table)
    else {
        return Err(format!("calendar timing value has no {}", table.table));
    };
    decode_rows(owned, bytes)
        .map(|rows| rows.len())
        .map_err(|error| format!("decode {}: {error}", table.table))
}

/// Summarizes a timing group value.
fn timing_from_value(value: &Value) -> Result<CalendarTimingSummary, String> {
    let row = RetainedRow::new(&EVENTS, &[(event_group::TIMING, value.clone())])?;
    let all_day = match row.columns.iter().find(|(name, _)| *name == "all_day") {
        Some((_, Field::Integer(flag))) => *flag != 0,
        _ => return Err("calendar timing value has no all_day".to_string()),
    };
    Ok(CalendarTimingSummary {
        start_time: required_text(&row, "start_time")?,
        end_time: required_text(&row, "end_time")?,
        timezone: required_text(&row, "timezone")?,
        all_day,
        rrule: column_text(&row, "rrule")?,
        repeat_until: column_text(&row, "repeat_until")?,
        excluded_dates: owned_count(&row, &EXDATES)?,
        added_dates: owned_count(&row, &RDATES)?,
        overrides: owned_count(&row, &OVERRIDES)?,
    })
}

fn value_read(
    field: CalendarConflictField,
    value: &Value,
) -> Result<CalendarConflictValue, String> {
    Ok(match field {
        CalendarConflictField::Timing => CalendarConflictValue::Timing(timing_from_value(value)?),
        _ => CalendarConflictValue::Text {
            text: text_from_value(value)?,
        },
    })
}

fn version_read(
    field: CalendarConflictField,
    version: &ConflictVersion,
    devices: &DeviceNames,
) -> Result<CalendarConflictVersion, String> {
    Ok(CalendarConflictVersion {
        version: version.writer.to_hex(),
        device: devices.describe(&version.device_id),
        edited_at_ms: version.clock.physical_ms(),
        displayed: version.displayed,
        value: value_read(field, &version.value)?,
    })
}

/// The sync space of the vault, or `None` for a vault without a person identity, which does
/// not sync.
async fn linked_sync_space(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
) -> Result<Option<SpaceId>, String> {
    Ok(local::space_anchor(tx)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .map(|_| SpaceId::personal(vault_id)))
}

/// Reads the open title, description, and timing conflicts of every event. A vault that does
/// not sync has none, so views can read them unconditionally.
pub async fn conflicts_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    devices: &DeviceNames,
) -> Result<CalendarConflictRead, CalendarConflictError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin calendar conflict read: {error}"))?;
    let mut conflicts = Vec::new();
    if let Some(space) = linked_sync_space(&mut tx, vault_id).await? {
        let open = Engine::vault()
            .open_conflicts(&mut tx, space, EVENTS_TABLE)
            .await
            .map_err(|error| format!("read calendar conflicts: {error}"))?;
        for row in open {
            for detail in row.groups {
                let Some(field) = CalendarConflictField::from_group(detail.group) else {
                    continue;
                };
                let versions = detail
                    .versions
                    .iter()
                    .map(|version| version_read(field, version, devices))
                    .collect::<Result<Vec<_>, _>>()?;
                conflicts.push(CalendarFieldConflict {
                    event_id: row.row_key.clone(),
                    field,
                    versions,
                });
            }
        }
    }
    tx.rollback()
        .await
        .map_err(|error| format!("end calendar conflict read: {error}"))?;
    Ok(CalendarConflictRead { conflicts })
}

/// The open conflict of `group` on event `event_id`.
async fn detail(
    tx: &mut Transaction<'_, Sqlite>,
    space: SpaceId,
    event_id: &str,
    group: GroupId,
) -> Result<ConflictDetail, CalendarConflictError> {
    Engine::vault()
        .conflict_details(tx, space, EVENTS_TABLE, event_id)
        .await
        .map_err(|error| format!("read calendar conflicts: {error}"))?
        .into_iter()
        .find(|detail| detail.group == group)
        .ok_or_else(|| CalendarConflictError::Resolved("the conflict is resolved".to_string()))
}

/// Resolves a conflict on an event field. The chosen value is written as a local edit and the
/// field is sealed even when its value did not change, so the write covers every version it
/// saw.
pub async fn resolve_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    request: CalendarConflictResolution,
    now_ms: u64,
) -> Result<(), CalendarConflictError> {
    let CalendarConflictResolution {
        event_id,
        field,
        choice,
    } = request;
    if event_id.trim().is_empty() {
        return Err("event_id is required".to_string().into());
    }
    let group = field.group();
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin calendar conflict resolution: {error}"))?;
    let space = linked_sync_space(&mut tx, vault_id)
        .await?
        .ok_or_else(|| CalendarConflictError::Resolved("this vault does not sync".to_string()))?;
    let detail = detail(&mut tx, space, &event_id, group).await?;
    if let CalendarConflictChoice::Version { version } = &choice {
        let writer = WriterId::from_hex(version);
        let chosen = detail
            .versions
            .iter()
            .find(|candidate| Some(candidate.writer) == writer)
            .ok_or_else(|| CalendarConflictError::Resolved("the version is gone".to_string()))?;
        write_version(&mut tx, &event_id, field, &chosen.value).await?;
    }
    Engine::vault()
        .force_group(&mut tx, EVENTS_TABLE, &event_id, group, now_ms)
        .await
        .map_err(|error| format!("mark calendar conflict resolved: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit calendar conflict resolution: {error}"))?;
    Ok(())
}

/// Writes a chosen version of `field` to event `event_id` and stamps its update time.
async fn write_version(
    conn: &mut SqliteConnection,
    event_id: &str,
    field: CalendarConflictField,
    value: &Value,
) -> Result<(), String> {
    let RetainedRow { columns, owned } =
        RetainedRow::new(&EVENTS, &[(field.group(), value.clone())])?;
    let updated_at = now(conn).await?;
    let mut assignments = Vec::with_capacity(columns.len() + 1);
    assignments.extend(columns);
    assignments.push((UPDATED_AT, Field::Text(updated_at)));
    let sql = format!(
        "UPDATE {} SET {} WHERE id = ?",
        EVENTS.name,
        assignments
            .iter()
            .map(|(column, _)| format!("{column} = ?"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for (_, value) in assignments {
        query = bind_field(query, value);
    }
    let result = query
        .bind(event_id)
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("write calendar conflict choice: {error}"))?;
    if result.rows_affected() != 1 {
        return Err("the event was not found".to_string());
    }
    if owned.is_empty() {
        return Ok(());
    }
    // Properties go before their overrides, which they reference.
    for table in [&OVERRIDE_PROPERTIES, &OVERRIDES, &RDATES, &EXDATES] {
        sqlx::query(AssertSqlSafe(format!(
            "DELETE FROM {} WHERE {} = ?",
            table.table, table.parent_column
        )))
        .bind(event_id)
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("replace {}: {error}", table.table))?;
    }
    // The timing group lists overrides before their properties.
    for (table, bytes) in owned {
        insert_owned(conn, table, event_id, &bytes).await?;
    }
    crate::sync_apply::refresh_event_projections(conn, &[event_id.to_string()])
        .await
        .map_err(|error| error.to_string())
}

/// Inserts the owned rows of a chosen timing version under `event_id`, keeping their ids and
/// clearing references to preserved components that are gone.
async fn insert_owned(
    conn: &mut SqliteConnection,
    owned: &'static OwnedTable,
    event_id: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let rows =
        decode_rows(owned, bytes).map_err(|error| format!("decode {}: {error}", owned.table))?;
    for (position, fields) in rows.into_iter().enumerate() {
        let mut columns: Vec<(&str, Field)> =
            vec![(owned.parent_column, Field::Text(event_id.to_string()))];
        let ordinal =
            i64::try_from(position).map_err(|_| format!("too many {} rows", owned.table))?;
        for column in owned.order.ordinal_columns() {
            columns.push((column, Field::Integer(ordinal)));
        }
        for (column, field) in owned.columns.iter().zip(fields) {
            let field = if column.name == COMPONENT_ID {
                live_reference(conn, "icalendar_components", Some(&field))
                    .await?
                    .map_or(Field::Null, Field::Text)
            } else {
                field
            };
            columns.push((column.name, field));
        }
        insert(conn, owned.table, columns).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
