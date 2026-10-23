//! Calendar collections: listing, creation, visibility, and removal with archive custody.

use std::sync::Arc;

use ganbaru_db::impl_sqlite_from_row;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::events::archive_or_delete_calendar_events_for_calendar;
use crate::events::scope::DeviceDate;

/// The built-in calendar seeded by the schema, which cannot be removed.
pub(crate) const BUILTIN_LOCAL_CALENDAR_ID: &str = "local";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarWrite {
    id: String,
    name: String,
    color: String,
    source: String,
    visible: bool,
    read_only: bool,
    source_url: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub struct CalendarRead {
    id: String,
    name: String,
    color: String,
    source: String,
    visible: i64,
    read_only: i64,
    source_url: Option<String>,
    last_synced: Option<String>,
    created_at: String,
    updated_at: String,
}
impl_sqlite_from_row!(CalendarRead {
    id,
    name,
    color,
    source,
    visible,
    read_only,
    source_url,
    last_synced,
    created_at,
    updated_at,
});

#[derive(Serialize)]
struct CountRow {
    cnt: i64,
}
impl_sqlite_from_row!(CountRow { cnt });

/// Lists every calendar by name.
pub async fn list_calendars(pool: &SqlitePool) -> Result<Vec<CalendarRead>, String> {
    sqlx::query_as::<_, CalendarRead>(
        "SELECT id, name, color, source, visible, read_only, source_url, last_synced, created_at, updated_at
         FROM calendars ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load calendars: {e}"))
}

/// Finds the calendar imported from an iCalendar file name.
pub async fn find_imported_calendar(
    pool: &SqlitePool,
    filename: String,
) -> Result<Option<CalendarRead>, String> {
    require_non_empty(&filename, "filename")?;
    sqlx::query_as::<_, CalendarRead>(
        "SELECT id, name, color, source, visible, read_only, source_url, last_synced, created_at, updated_at
         FROM calendars WHERE source = 'ics' AND source_url = ? LIMIT 1",
    )
    .bind(filename)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("find imported calendar: {e}"))
}

/// Counts the events stored in one calendar.
pub async fn count_events(pool: &SqlitePool, calendar_id: String) -> Result<i64, String> {
    require_non_empty(&calendar_id, "calendar_id")?;
    let row = sqlx::query_as::<_, CountRow>(
        "SELECT COUNT(*) as cnt FROM calendar_events WHERE calendar_id = ?",
    )
    .bind(calendar_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("count calendar events: {e}"))?;
    Ok(row.cnt)
}

/// Creates a calendar.
pub async fn add_calendar(pool: &SqlitePool, calendar: CalendarWrite) -> Result<(), String> {
    validate_calendar_write(&calendar)?;
    sqlx::query(
        "INSERT INTO calendars
           (id, name, color, source, visible, read_only, source_url, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(calendar.id)
    .bind(calendar.name)
    .bind(calendar.color)
    .bind(calendar.source)
    .bind(if calendar.visible { 1_i64 } else { 0_i64 })
    .bind(if calendar.read_only { 1_i64 } else { 0_i64 })
    .bind(calendar.source_url)
    .bind(calendar.created_at)
    .bind(calendar.updated_at)
    .execute(pool)
    .await
    .map_err(|e| format!("insert calendar: {e}"))?;
    Ok(())
}

/// Shows or hides a calendar.
pub async fn set_visibility(
    pool: &SqlitePool,
    id: String,
    visible: bool,
    updated_at: String,
) -> Result<(), String> {
    require_non_empty(&id, "id")?;
    require_non_empty(&updated_at, "updated_at")?;
    let result = sqlx::query("UPDATE calendars SET visible = ?, updated_at = ? WHERE id = ?")
        .bind(if visible { 1_i64 } else { 0_i64 })
        .bind(updated_at)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("update calendar visibility: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("calendar not found".to_string());
    }
    Ok(())
}

/// Removes a calendar, archiving protected history and keeping archived import graphs.
pub async fn remove_calendar(
    pool: &SqlitePool,
    device: Arc<dyn DeviceDate>,
    id: String,
) -> Result<(), String> {
    require_non_empty(&id, "id")?;
    if id == BUILTIN_LOCAL_CALENDAR_ID {
        return Err("The built-in local calendar cannot be removed".into());
    }
    let epoch_ms = jiff::Timestamp::now().as_millisecond();
    let permit = crate::events::scope::SCOPE_GATE
        .try_acquire()
        .map_err(|_| "A Calendar operation is being reviewed; retry after it finishes")?;
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        device.local_date(epoch_ms)
    });
    let today = tokio::time::timeout(crate::events::scope::SCOPE_WORKER_TIMEOUT, worker)
        .await
        .map_err(|_| "Calendar removal device-date lookup timed out; retry after it finishes")?
        .map_err(|error| format!("Calendar removal device-date worker: {error}"))??;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    archive_or_delete_calendar_events_for_calendar(
        &mut tx,
        Some(&id),
        crate::recurrence::canonical::ScopeClock {
            epoch_ms,
            floating_today: Some(today),
        },
    )
    .await?;
    retain_archived_imports_tx(&mut tx, &id).await?;
    let result = sqlx::query("DELETE FROM calendars WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("delete calendar: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("calendar not found".to_string());
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Keep archived import graphs when their original calendar is removed. Local
/// storage custody changes; the archive retains its original calendar identity.
pub(crate) async fn retain_archived_imports_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    calendar_id: &str,
) -> Result<(), String> {
    let mut budget = crate::events::ReadBudget::default();
    let objects = budget.read::<(String,)>(tx, calendar_id,
        "SELECT object.id FROM icalendar_objects object WHERE object.calendar_id=?1 AND (
            EXISTS(SELECT 1 FROM calendar_event_archive_import_objects owner WHERE owner.object_id=object.id))",
        &["id"]).await?;
    if objects.is_empty() {
        return Ok(());
    }
    let objects = serde_json::to_string(&objects.into_iter().map(|row| row.0).collect::<Vec<_>>())
        .map_err(|error| format!("encode archived calendar imports: {error}"))?;
    // This shares the complete object/component allocation allowance. A large
    // retained graph fails before calendar deletion can cascade away its history.
    budget.read::<(String,)>(tx, &objects,
        "SELECT id FROM icalendar_components WHERE object_id IN (SELECT value FROM json_each(?1))", &["id"]).await?;
    sqlx::query("UPDATE icalendar_components SET calendar_id=?1 WHERE object_id IN (SELECT value FROM json_each(?2))")
        .bind(BUILTIN_LOCAL_CALENDAR_ID).bind(&objects).execute(&mut **tx).await
        .map_err(|error| format!("retain archived calendar components: {error}"))?;
    sqlx::query(
        "UPDATE icalendar_objects SET calendar_id=?1 WHERE id IN (SELECT value FROM json_each(?2))",
    )
    .bind(BUILTIN_LOCAL_CALENDAR_ID)
    .bind(objects)
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("retain archived calendar objects: {error}"))?;
    Ok(())
}

fn validate_calendar_write(calendar: &CalendarWrite) -> Result<(), String> {
    require_non_empty(&calendar.id, "id")?;
    require_non_empty(&calendar.name, "name")?;
    require_non_empty(&calendar.source, "source")?;
    require_non_empty(&calendar.created_at, "created_at")?;
    require_non_empty(&calendar.updated_at, "updated_at")
}

fn require_non_empty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} cannot be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CalendarWrite, validate_calendar_write};

    fn calendar() -> CalendarWrite {
        CalendarWrite {
            id: "cal".to_string(),
            name: "Calendar".to_string(),
            color: "".to_string(),
            source: "local".to_string(),
            visible: true,
            read_only: false,
            source_url: None,
            created_at: "2026-05-01 00:00:00".to_string(),
            updated_at: "2026-05-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn validates_calendar_write_identity() {
        assert!(validate_calendar_write(&calendar()).is_ok());
        let mut invalid = calendar();
        invalid.id = " ".to_string();
        assert!(validate_calendar_write(&invalid).is_err());
    }
}
