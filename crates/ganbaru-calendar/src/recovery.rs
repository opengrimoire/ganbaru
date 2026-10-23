//! Restoring Calendar rows offered for sync recovery.
//!
//! A deleted event comes back as a new event with its retained values, inserted as an ordinary
//! local write so the restore seals like any edit. It gets a new id, since a deleted key cannot
//! be inserted again, and so do its owned rows that carry an id of their own, so two devices
//! restoring the same offer never write the same child id. Its calendar becomes the built-in
//! Local calendar when the retained one is gone, and references to a project or a preserved
//! iCalendar component that are gone are cleared. The retained source uid is kept unless a live
//! event of the same calendar already holds it. `updated_at` is the restore time. Task links
//! among the deleted descendants follow the new event when their task still exists.
//!
//! A removed calendar does not come back: its deleted events are restored into Local, which is
//! where an event added on one device to a calendar another device removed belongs. Preserved
//! iCalendar objects and components are source data an import writes, so they come back by
//! importing the file again rather than through recovery.

use std::collections::HashMap;

use ganbaru_sync::RecoveryDescendant;
use ganbaru_sync::domains::calendar::decode_rows;
use ganbaru_sync::manifest::vault::calendar::{
    EVENTS, EVENTS_TABLE, OVERRIDE_PROPERTIES, OVERRIDES, TASK_EVENT_LINKS, TASK_EVENT_LINKS_TABLE,
};
use ganbaru_sync::manifest::{OwnedTable, TableSpec};
use ganbaru_sync_contracts::{Field, GroupId, Value};
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};

use crate::calendars::BUILTIN_LOCAL_CALENDAR_ID;

/// Column of a row's own id in the event and its owned tables.
const ID: &str = "id";

/// Column every event and override stamps with its last write.
pub(crate) const UPDATED_AT: &str = "updated_at";

/// Column naming a preserved iCalendar component, on events and their owned rows.
pub(crate) const COMPONENT_ID: &str = "icalendar_component_id";

/// Column of an override property naming its override.
const OVERRIDE_ID: &str = "override_id";

/// Retained values of one recovered row, split into its columns and its owned row lists.
pub(crate) struct RetainedRow {
    pub(crate) columns: Vec<(&'static str, Field)>,
    pub(crate) owned: Vec<(&'static OwnedTable, Vec<u8>)>,
}

impl RetainedRow {
    /// Maps retained group values of `spec` to columns and owned lists in manifest order.
    pub(crate) fn new(
        spec: &'static TableSpec,
        values: &[(GroupId, Value)],
    ) -> Result<Self, String> {
        let mut columns = Vec::new();
        let mut owned = Vec::new();
        for (group_id, value) in values {
            if spec.is_exists_group(*group_id) {
                continue;
            }
            let group = spec
                .group(*group_id)
                .ok_or_else(|| format!("{} has no field group {}", spec.name, group_id.get()))?;
            let fields = value.fields();
            if fields.len() != group.field_count() {
                return Err(format!(
                    "a recovered {} {} value has the wrong shape",
                    spec.name, group.name
                ));
            }
            let (column_fields, owned_fields) = fields.split_at(group.columns().len());
            for (column, field) in group.columns().iter().zip(column_fields) {
                columns.push((column.name, field.clone()));
            }
            for (table, field) in group.owned_tables().iter().zip(owned_fields) {
                let Field::Blob(bytes) = field else {
                    return Err(format!(
                        "a recovered {} owned value is not encoded",
                        spec.name
                    ));
                };
                owned.push((table, bytes.clone()));
            }
        }
        Ok(Self { columns, owned })
    }

    fn text(&self, column: &str) -> Option<&str> {
        self.columns
            .iter()
            .find(|(name, _)| *name == column)
            .and_then(|(_, field)| match field {
                Field::Text(text) => Some(text.as_str()),
                _ => None,
            })
    }

    fn set(&mut self, column: &'static str, field: Field) {
        match self.columns.iter_mut().find(|(name, _)| *name == column) {
            Some((_, existing)) => *existing = field,
            None => self.columns.push((column, field)),
        }
    }
}

/// A new random id in the form Calendar gives generated rows.
async fn new_id(conn: &mut SqliteConnection) -> Result<String, String> {
    sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
        .fetch_one(&mut *conn)
        .await
        .map_err(|error| format!("generate a Calendar id: {error}"))
}

/// The current time in the stored timestamp form.
pub(crate) async fn now(conn: &mut SqliteConnection) -> Result<String, String> {
    sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
        .fetch_one(&mut *conn)
        .await
        .map_err(|error| format!("read current time: {error}"))
}

/// Whether a row keyed `key` exists in `table`.
async fn exists(conn: &mut SqliteConnection, table: &str, key: &str) -> Result<bool, String> {
    sqlx::query_scalar::<_, i64>(AssertSqlSafe(format!("SELECT 1 FROM {table} WHERE id = ?")))
        .bind(key)
        .fetch_optional(&mut *conn)
        .await
        .map(|row| row.is_some())
        .map_err(|error| format!("look up {table} row: {error}"))
}

/// The referenced id when it still names a row of `table`.
pub(crate) async fn live_reference(
    conn: &mut SqliteConnection,
    table: &str,
    field: Option<&Field>,
) -> Result<Option<String>, String> {
    match field {
        Some(Field::Text(id)) if exists(conn, table, id).await? => Ok(Some(id.clone())),
        _ => Ok(None),
    }
}

pub(crate) fn bind_field<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    field: Field,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    match field {
        Field::Null => query.bind(None::<String>),
        Field::Integer(value) => query.bind(value),
        Field::Text(text) => query.bind(text),
        Field::Blob(bytes) => query.bind(bytes),
    }
}

/// Inserts `columns` into `table`.
pub(crate) async fn insert(
    conn: &mut SqliteConnection,
    table: &str,
    columns: Vec<(&str, Field)>,
) -> Result<(), String> {
    let names: Vec<&str> = columns.iter().map(|(name, _)| *name).collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        vec!["?"; names.len()].join(", ")
    );
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for (_, field) in columns {
        query = bind_field(query, field);
    }
    query
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("restore {table} row: {error}"))?;
    Ok(())
}

/// Restores a deleted event and the task links among its deleted descendants on a connection
/// inside the caller's transaction, and returns the id of the new event. Other descendants are
/// left out: an event has no other cascade children.
pub async fn restore_recovered_event(
    conn: &mut SqliteConnection,
    row_key: &str,
    values: &[(GroupId, Value)],
    descendants: &[RecoveryDescendant],
) -> Result<String, String> {
    let event_id = restore_event(conn, values).await?;
    for descendant in descendants {
        if descendant.table == TASK_EVENT_LINKS_TABLE {
            restore_task_link(conn, row_key, &event_id, &descendant.values).await?;
        }
    }
    crate::sync_apply::refresh_event_projections(conn, std::slice::from_ref(&event_id))
        .await
        .map_err(|error| error.to_string())?;
    Ok(event_id)
}

/// Restores the deleted events of a removed calendar into Local, each with its task links, and
/// returns the id of the first restored event. An event that can no longer be inserted is left
/// out; a calendar offer without events has nothing to restore and stays discardable.
pub async fn restore_recovered_calendar(
    conn: &mut SqliteConnection,
    descendants: &[RecoveryDescendant],
) -> Result<String, String> {
    let mut first = None;
    for event in descendants
        .iter()
        .filter(|descendant| descendant.table == EVENTS_TABLE)
    {
        let links: Vec<RecoveryDescendant> = descendants
            .iter()
            .filter(|descendant| {
                descendant.table == TASK_EVENT_LINKS_TABLE
                    && link_event(&descendant.values).as_deref() == Some(event.row_key.as_str())
            })
            .cloned()
            .collect();
        let mut savepoint = conn
            .begin()
            .await
            .map_err(|error| format!("begin restore event: {error}"))?;
        match restore_recovered_event(&mut savepoint, &event.row_key, &event.values, &links).await {
            Ok(id) => {
                savepoint
                    .commit()
                    .await
                    .map_err(|error| format!("commit restore event: {error}"))?;
                first.get_or_insert(id);
            }
            Err(_) => savepoint
                .rollback()
                .await
                .map_err(|error| format!("roll back restore event: {error}"))?,
        }
    }
    first.ok_or_else(|| {
        "a removed calendar comes back by adding or subscribing to it again".to_string()
    })
}

/// Inserts a new event from retained values and returns its id.
async fn restore_event(
    conn: &mut SqliteConnection,
    values: &[(GroupId, Value)],
) -> Result<String, String> {
    let mut row = RetainedRow::new(&EVENTS, values)?;
    let event_id = new_id(conn).await?;
    row.set(ID, Field::Text(event_id.clone()));
    let calendar = live_reference(conn, "calendars", field(&row, "calendar_id"))
        .await?
        .unwrap_or_else(|| BUILTIN_LOCAL_CALENDAR_ID.to_string());
    row.set("calendar_id", Field::Text(calendar.clone()));
    let project = live_reference(conn, "projects", field(&row, "project_id")).await?;
    row.set("project_id", project.map_or(Field::Null, Field::Text));
    let component = live_reference(conn, "icalendar_components", field(&row, COMPONENT_ID)).await?;
    row.set(COMPONENT_ID, component.map_or(Field::Null, Field::Text));
    if let Some(uid) = row.text("source_uid").map(str::to_string)
        && uid_taken(conn, &calendar, &uid).await?
    {
        row.set("source_uid", Field::Null);
    }
    let now = now(conn).await?;
    row.set(UPDATED_AT, Field::Text(now.clone()));

    let RetainedRow { columns, owned } = row;
    insert(conn, EVENTS.name, columns).await?;
    let mut override_ids = HashMap::new();
    for (table, bytes) in owned {
        insert_owned(conn, table, &event_id, &bytes, &now, &mut override_ids).await?;
    }
    Ok(event_id)
}

fn field<'a>(row: &'a RetainedRow, column: &str) -> Option<&'a Field> {
    row.columns
        .iter()
        .find(|(name, _)| *name == column)
        .map(|(_, field)| field)
}

/// Whether a live event of `calendar` already holds the source uid.
async fn uid_taken(conn: &mut SqliteConnection, calendar: &str, uid: &str) -> Result<bool, String> {
    sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM calendar_events WHERE calendar_id = ? AND source_uid = ?",
    )
    .bind(calendar)
    .bind(uid)
    .fetch_optional(&mut *conn)
    .await
    .map(|row| row.is_some())
    .map_err(|error| format!("look up Calendar source uid: {error}"))
}

/// Inserts the owned rows of a restored event under `event_id`. Rows get new ids, override
/// properties follow their override's new id, and references to preserved components that are
/// gone are cleared. Overrides precede their properties in the timing group, so their new ids
/// are known by then.
async fn insert_owned(
    conn: &mut SqliteConnection,
    owned: &'static OwnedTable,
    event_id: &str,
    bytes: &[u8],
    now: &str,
    override_ids: &mut HashMap<String, String>,
) -> Result<(), String> {
    let rows = decode_rows(owned, bytes)
        .map_err(|error| format!("decode recovered {}: {error}", owned.table))?;
    for (position, fields) in rows.into_iter().enumerate() {
        let mut columns: Vec<(&str, Field)> =
            vec![(owned.parent_column, Field::Text(event_id.to_string()))];
        let ordinal = i64::try_from(position)
            .map_err(|_| format!("too many recovered {} rows", owned.table))?;
        for column in owned.order.ordinal_columns() {
            columns.push((column, Field::Integer(ordinal)));
        }
        for (column, field) in owned.columns.iter().zip(fields) {
            let field = match column.name {
                ID => {
                    let id = new_id(conn).await?;
                    if owned.table == OVERRIDES.table
                        && let Field::Text(old) = &field
                    {
                        override_ids.insert(old.clone(), id.clone());
                    }
                    Field::Text(id)
                }
                OVERRIDE_ID if owned.table == OVERRIDE_PROPERTIES.table => {
                    let Field::Text(old) = &field else {
                        return Err("a recovered override property names no override".to_string());
                    };
                    let id = override_ids.get(old).ok_or_else(|| {
                        "a recovered override property names an unknown override".to_string()
                    })?;
                    Field::Text(id.clone())
                }
                COMPONENT_ID => {
                    let live = live_reference(conn, "icalendar_components", Some(&field)).await?;
                    live.map_or(Field::Null, Field::Text)
                }
                UPDATED_AT => Field::Text(now.to_string()),
                _ => field,
            };
            columns.push((column.name, field));
        }
        insert(conn, owned.table, columns).await?;
    }
    Ok(())
}

/// The event a retained task link names.
fn link_event(values: &[(GroupId, Value)]) -> Option<String> {
    let link = RetainedRow::new(&TASK_EVENT_LINKS, values).ok()?;
    link.text("event_id").map(str::to_string)
}

/// Inserts a retained task link of the deleted event `old_event` for the restored event, when
/// it names that event and its task still exists.
async fn restore_task_link(
    conn: &mut SqliteConnection,
    old_event: &str,
    event_id: &str,
    values: &[(GroupId, Value)],
) -> Result<(), String> {
    let link = RetainedRow::new(&TASK_EVENT_LINKS, values)?;
    if link.text("event_id") != Some(old_event) {
        return Ok(());
    }
    let Some(task_id) = live_reference(conn, "project_tasks", field(&link, "task_id")).await?
    else {
        return Ok(());
    };
    let key = ganbaru_projects::keys::task_event_key(&task_id, event_id)?;
    let created_at = match link.text("created_at") {
        Some(created_at) => created_at.to_string(),
        None => now(conn).await?,
    };
    let kind = link
        .text("link_kind")
        .ok_or_else(|| "a recovered task link has no kind".to_string())?
        .to_string();
    insert(
        conn,
        TASK_EVENT_LINKS.name,
        vec![
            ("task_id", Field::Text(task_id)),
            ("event_id", Field::Text(event_id.to_string())),
            ("link_kind", Field::Text(kind)),
            ("created_at", Field::Text(created_at)),
            (TASK_EVENT_LINKS.key_column, Field::Text(key)),
        ],
    )
    .await
}
