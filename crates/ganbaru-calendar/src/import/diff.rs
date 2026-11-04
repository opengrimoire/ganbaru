//! Import writes of one event and its owned rows as a diff against what the vault holds.
//!
//! Re-importing a source keeps every stored row whose content the source still carries, with its
//! id, and writes only the rows that differ. An unchanged source therefore changes no row, and a
//! row another replica added concurrently is not overwritten by an identical copy of the rest.

use std::collections::{HashMap, HashSet, VecDeque};

use ganbaru_sync_contracts::Field;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

use super::preservation::preservation_link_id;
use super::{
    CalendarImportEvent, parse_i64_list, parse_organizer, parse_string_list, parse_string_map,
};
use crate::description::{
    sanitize_calendar_description_html, sanitize_optional_calendar_description,
};
use crate::geo::parse_geo;
use crate::recovery::bind_field;

/// How an import writes the preservation link columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LinkMode {
    /// The import stores new preserved components, so every row links to the component it
    /// projects.
    Relink,
    /// The stored preserved components stay, so a matched row keeps its link and a new row has
    /// none.
    Keep,
}

/// An owned table of an event, its columns after the owner column, and how import matches rows.
struct ChildTable {
    table: &'static str,
    owner: &'static str,
    columns: &'static [&'static str],
    /// Primary key columns after the owner.
    key: &'static [usize],
    /// Columns that make two rows the same item, so the stored row keeps its id. A table whose
    /// identity is its key holds each identity at most once.
    identity: &'static [usize],
    /// Row id column, minted on insert when an unmatched row carries none.
    id: Option<usize>,
    /// Preservation link columns.
    links: &'static [usize],
    /// Creation and update timestamp columns: kept on an unchanged row, the update stamp renewed on
    /// a changed one.
    timestamps: Option<(usize, usize)>,
}

static NOTIFICATIONS: ChildTable = ChildTable {
    table: "calendar_event_notifications",
    owner: "event_id",
    columns: &["id", "offset_minutes", "sort_order"],
    key: &[0],
    identity: &[1],
    id: Some(0),
    links: &[],
    timestamps: None,
};

static EXDATES: ChildTable = ChildTable {
    table: "calendar_event_exdates",
    owner: "event_id",
    columns: &["occurrence_date", "id"],
    key: &[0],
    identity: &[0],
    id: Some(1),
    links: &[],
    timestamps: None,
};

static CATEGORIES: ChildTable = ChildTable {
    table: "calendar_event_categories",
    owner: "event_id",
    columns: &["id", "category", "sort_order"],
    key: &[0],
    identity: &[1],
    id: Some(0),
    links: &[],
    timestamps: None,
};

static RDATES: ChildTable = ChildTable {
    table: "calendar_event_rdates",
    owner: "event_id",
    columns: &["occurrence_start", "id"],
    key: &[0],
    identity: &[0],
    id: Some(1),
    links: &[],
    timestamps: None,
};

static EXTENDED_PROPERTIES: ChildTable = ChildTable {
    table: "calendar_event_extended_properties",
    owner: "event_id",
    columns: &["property_key", "id", "property_value", "sort_order"],
    key: &[0],
    identity: &[0],
    id: Some(1),
    links: &[],
    timestamps: None,
};

static ORGANIZERS: ChildTable = ChildTable {
    table: "calendar_event_organizers",
    owner: "event_id",
    columns: &["ordinal", "name", "email"],
    key: &[0],
    identity: &[0],
    id: None,
    links: &[],
    timestamps: None,
};

static ATTENDEES: ChildTable = ChildTable {
    table: "calendar_event_attendees",
    owner: "event_id",
    columns: &[
        "id",
        "name",
        "email",
        "role",
        "status",
        "rsvp",
        "sort_order",
        "icalendar_component_id",
        "icalendar_property_index",
    ],
    key: &[0],
    identity: &[1, 2, 3, 4, 5],
    id: Some(0),
    links: &[7, 8],
    timestamps: None,
};

static ALARMS: ChildTable = ChildTable {
    table: "calendar_event_alarms",
    owner: "event_id",
    columns: &[
        "id",
        "action",
        "trigger_type",
        "trigger_value",
        "description",
        "sort_order",
        "icalendar_component_id",
    ],
    key: &[0],
    identity: &[1, 2, 3, 4],
    id: Some(0),
    links: &[6],
    timestamps: None,
};

static OVERRIDES: ChildTable = ChildTable {
    table: "calendar_event_overrides",
    owner: "parent_event_id",
    columns: &[
        "recurrence_id",
        "id",
        "recurrence_range",
        "title",
        "start_time",
        "end_time",
        "description",
        "location",
        "url",
        "color",
        "status",
        "transparency",
        "visibility",
        "icalendar_component_id",
        "created_at",
        "updated_at",
    ],
    key: &[0],
    identity: &[0],
    id: Some(1),
    links: &[13],
    timestamps: Some((14, 15)),
};

static OVERRIDE_PROPERTIES: ChildTable = ChildTable {
    table: "calendar_event_override_extended_properties",
    owner: "event_id",
    columns: &[
        "override_id",
        "property_key",
        "id",
        "property_value",
        "sort_order",
    ],
    key: &[0, 1],
    identity: &[0, 1],
    id: Some(2),
    links: &[],
    timestamps: None,
};

/// Event columns an import writes, with the preservation link last.
const EVENT_COLUMNS: &[&str] = &[
    "title",
    "start_time",
    "end_time",
    "timezone",
    "color",
    "description",
    "rrule",
    "repeat_until",
    "all_day",
    "location",
    "url",
    "transparency",
    "status",
    "visibility",
    "priority",
    "geo_lat",
    "geo_lng",
    "sequence",
    "meeting_enabled",
    "guest_can_modify",
    "guest_can_invite_others",
    "guest_can_see_other_guests",
    "icalendar_component_id",
];
const EVENT_LINK: usize = EVENT_COLUMNS.len() - 1;

/// The rows one owned table loses and gains.
struct TableDiff {
    table: &'static ChildTable,
    /// Primary key values, after the owner, of stored rows to delete.
    deletes: Vec<Vec<Field>>,
    /// Full rows to insert.
    inserts: Vec<Vec<Field>>,
}

/// What importing one event writes, decided before anything is written.
pub(super) struct EventPlan {
    event_id: String,
    /// Whether the event is new to the vault.
    insert: bool,
    /// Event columns to write, or `None` when the stored row and its owned rows stay as they are.
    event_row: Option<Vec<Field>>,
    tables: Vec<TableDiff>,
    /// Stored id of each alarm and override that matched a stored row, by payload id.
    reused_ids: HashMap<String, String>,
}

impl EventPlan {
    /// Whether the plan writes any row.
    pub(super) fn changes(&self) -> bool {
        self.event_row.is_some()
    }

    /// Whether the plan inserts the event.
    pub(super) fn inserts(&self) -> bool {
        self.insert
    }

    /// The vault id of the alarm or override the payload calls `payload_id`.
    pub(super) fn row_id<'a>(&'a self, payload_id: &'a str) -> &'a str {
        self.reused_ids
            .get(payload_id)
            .map_or(payload_id, String::as_str)
    }
}

/// Plans importing `event` as the vault event `event_id`, reading its stored rows when `stored`.
pub(super) async fn plan_event(
    conn: &mut SqliteConnection,
    target_calendar_id: &str,
    event_id: &str,
    stored: bool,
    event: &CalendarImportEvent,
    mode: LinkMode,
    now: &str,
) -> Result<EventPlan, String> {
    let mut desired_event = event_fields(event, mode)?;
    let stored_event = if stored {
        let row = stored_event(conn, target_calendar_id, event_id).await?;
        if mode == LinkMode::Keep {
            desired_event[EVENT_LINK] = row[EVENT_LINK].clone();
        }
        Some(row)
    } else {
        None
    };

    let mut plan = EventPlan {
        event_id: event_id.to_string(),
        insert: !stored,
        event_row: None,
        tables: Vec::new(),
        reused_ids: HashMap::new(),
    };
    let lists = [
        (&NOTIFICATIONS, notification_rows(event)?),
        (&EXDATES, string_rows(&event.exceptions, "exceptions")?),
        (&CATEGORIES, category_rows(event)?),
        (&RDATES, string_rows(&event.rdate, "rdate")?),
        (&EXTENDED_PROPERTIES, extended_property_rows(event)?),
        (&ORGANIZERS, organizer_rows(event)?),
        (&ATTENDEES, attendee_rows(event, mode)),
        (&ALARMS, alarm_rows(event, mode)),
        (&OVERRIDES, override_rows(event, mode)),
    ];
    let mut override_ids = Vec::new();
    for (table, desired) in lists {
        let stored_rows = if stored {
            stored_rows(conn, table, event_id).await?
        } else {
            Vec::new()
        };
        let (diff, resolved) = diff_rows(table, stored_rows, desired, mode, now);
        if let Some(id) = table.id {
            for (payload, row) in resolved.iter() {
                if let (Some(payload_id), Field::Text(row_id)) = (payload, &row[id])
                    && payload_id != row_id
                {
                    plan.reused_ids.insert(payload_id.clone(), row_id.clone());
                }
            }
        }
        if table.table == OVERRIDES.table {
            override_ids = resolved
                .into_iter()
                .filter_map(|(payload, row)| payload.zip(text_of(&row[1])))
                .collect();
        }
        plan.tables.push(diff);
    }
    let stored_properties = if stored {
        stored_rows(conn, &OVERRIDE_PROPERTIES, event_id).await?
    } else {
        Vec::new()
    };
    let (diff, _) = diff_rows(
        &OVERRIDE_PROPERTIES,
        stored_properties,
        override_property_rows(event, &override_ids)?,
        mode,
        now,
    );
    plan.tables.push(diff);

    let children_change = plan
        .tables
        .iter()
        .any(|diff| !diff.deletes.is_empty() || !diff.inserts.is_empty());
    let row_changes = stored_event.as_ref() != Some(&desired_event);
    if row_changes || children_change {
        plan.event_row = Some(desired_event);
    }
    Ok(plan)
}

/// Writes a planned event: its row, then its owned rows, deletes before inserts.
pub(super) async fn write_event(
    conn: &mut SqliteConnection,
    target_calendar_id: &str,
    now: &str,
    event: &CalendarImportEvent,
    plan: EventPlan,
) -> Result<(), String> {
    let Some(row) = plan.event_row else {
        return Ok(());
    };
    if plan.insert {
        insert_event(conn, target_calendar_id, now, event, &plan.event_id, row).await?;
    } else {
        update_event(conn, target_calendar_id, now, &plan.event_id, row).await?;
    }
    for diff in &plan.tables {
        delete_rows(conn, diff, &plan.event_id).await?;
    }
    for diff in plan.tables {
        insert_rows(conn, &diff, &plan.event_id).await?;
    }
    Ok(())
}

/// A desired row as written, with the payload id it carried.
type WrittenRow = (Option<String>, Vec<Field>);

/// Matches desired rows to stored rows by identity. A matched row takes the stored id, and in
/// [`LinkMode::Keep`] the stored links; rows equal to their stored match are left alone. Returns
/// the diff and every desired row as written, with the payload id it carried.
fn diff_rows(
    table: &'static ChildTable,
    stored: Vec<Vec<Field>>,
    desired: Vec<Vec<Field>>,
    mode: LinkMode,
    now: &str,
) -> (TableDiff, Vec<WrittenRow>) {
    let unique = table.identity == table.key;
    let mut seen = HashSet::new();
    let mut index: HashMap<Vec<Field>, VecDeque<usize>> = HashMap::new();
    for (position, row) in stored.iter().enumerate() {
        index
            .entry(pick(row, table.identity))
            .or_default()
            .push_back(position);
    }
    let mut kept = vec![false; stored.len()];
    let mut inserts = Vec::new();
    let mut resolved = Vec::new();
    for mut row in desired {
        let identity = pick(&row, table.identity);
        if unique && !seen.insert(identity.clone()) {
            continue;
        }
        let payload_id = table.id.and_then(|id| text_of(&row[id]));
        match index.get_mut(&identity).and_then(VecDeque::pop_front) {
            Some(position) => {
                let old = &stored[position];
                if let Some(id) = table.id {
                    row[id] = old[id].clone();
                }
                if mode == LinkMode::Keep {
                    for &link in table.links {
                        row[link] = old[link].clone();
                    }
                }
                if let Some((created, updated)) = table.timestamps {
                    row[created] = old[created].clone();
                    row[updated] = old[updated].clone();
                }
                if row == *old {
                    kept[position] = true;
                } else {
                    if let Some((_, updated)) = table.timestamps {
                        row[updated] = Field::Text(now.to_string());
                    }
                    inserts.push(row.clone());
                }
            }
            None => {
                if mode == LinkMode::Keep {
                    for &link in table.links {
                        row[link] = Field::Null;
                    }
                }
                if let Some((created, updated)) = table.timestamps {
                    row[created] = Field::Text(now.to_string());
                    row[updated] = Field::Text(now.to_string());
                }
                inserts.push(row.clone());
            }
        }
        resolved.push((payload_id, row));
    }
    let deletes = stored
        .iter()
        .zip(kept)
        .filter(|(_, kept)| !kept)
        .map(|(row, _)| pick(row, table.key))
        .collect();
    (
        TableDiff {
            table,
            deletes,
            inserts,
        },
        resolved,
    )
}

fn pick(row: &[Field], columns: &[usize]) -> Vec<Field> {
    columns.iter().map(|&column| row[column].clone()).collect()
}

fn text_of(field: &Field) -> Option<String> {
    match field {
        Field::Text(text) => Some(text.clone()),
        _ => None,
    }
}

fn text(value: &str) -> Field {
    Field::Text(value.to_string())
}

fn optional_text(value: Option<&str>) -> Field {
    value.map_or(Field::Null, text)
}

fn optional_integer(value: Option<i64>) -> Field {
    value.map_or(Field::Null, Field::Integer)
}

fn flag(value: bool) -> Field {
    Field::Integer(i64::from(value))
}

fn order(position: usize) -> Field {
    Field::Integer(position as i64)
}

/// The preservation link of a row in `mode`; [`LinkMode::Keep`] fills it from the stored match.
fn link(component_id: &Option<String>, mode: LinkMode) -> Field {
    optional_text(preservation_link_id(component_id, mode == LinkMode::Relink))
}

fn event_fields(event: &CalendarImportEvent, mode: LinkMode) -> Result<Vec<Field>, String> {
    let geo = parse_geo(&event.geo)?;
    Ok(vec![
        text(&event.title),
        text(&event.start_time),
        text(&event.end_time),
        text(&event.timezone),
        optional_integer(event.color),
        Field::Text(sanitize_calendar_description_html(&event.description)),
        optional_text(event.rrule.as_deref()),
        optional_text(event.repeat_until.as_deref()),
        flag(event.all_day),
        text(&event.location),
        text(&event.url),
        text(&event.transparency),
        text(&event.status),
        text(&event.visibility),
        optional_integer(event.priority),
        optional_integer(geo.map(|value| value.lat)),
        optional_integer(geo.map(|value| value.lng)),
        Field::Integer(event.sequence),
        flag(event.meeting_enabled),
        flag(event.guest_can_modify),
        flag(event.guest_can_invite_others),
        flag(event.guest_can_see_other_guests),
        link(&event.icalendar_component_id, mode),
    ])
}

fn notification_rows(event: &CalendarImportEvent) -> Result<Vec<Vec<Field>>, String> {
    Ok(parse_i64_list(&event.notifications, "notifications")?
        .into_iter()
        .enumerate()
        .map(|(position, offset)| vec![Field::Null, Field::Integer(offset), order(position)])
        .collect())
}

fn string_rows(value: &Option<String>, field: &str) -> Result<Vec<Vec<Field>>, String> {
    Ok(parse_string_list(value, field)?
        .into_iter()
        .map(|date| vec![Field::Text(date), Field::Null])
        .collect())
}

fn category_rows(event: &CalendarImportEvent) -> Result<Vec<Vec<Field>>, String> {
    Ok(parse_string_list(&event.categories, "categories")?
        .into_iter()
        .enumerate()
        .map(|(position, category)| vec![Field::Null, Field::Text(category), order(position)])
        .collect())
}

fn extended_property_rows(event: &CalendarImportEvent) -> Result<Vec<Vec<Field>>, String> {
    Ok(
        parse_string_map(&event.extended_properties, "extended_properties")?
            .into_iter()
            .enumerate()
            .map(|(position, (key, value))| {
                vec![
                    Field::Text(key),
                    Field::Null,
                    Field::Text(value),
                    order(position),
                ]
            })
            .collect(),
    )
}

fn organizer_rows(event: &CalendarImportEvent) -> Result<Vec<Vec<Field>>, String> {
    Ok(parse_organizer(&event.organizer)?
        .into_iter()
        .map(|organizer| {
            vec![
                Field::Integer(0),
                organizer.name.map_or(Field::Null, Field::Text),
                Field::Text(organizer.email),
            ]
        })
        .collect())
}

fn attendee_rows(event: &CalendarImportEvent, mode: LinkMode) -> Vec<Vec<Field>> {
    event
        .attendees
        .iter()
        .enumerate()
        .map(|(position, attendee)| {
            let index = if mode == LinkMode::Relink {
                optional_integer(attendee.icalendar_property_index)
            } else {
                Field::Null
            };
            vec![
                text(&attendee.id),
                optional_text(attendee.name.as_deref()),
                text(&attendee.email),
                text(&attendee.role),
                text(&attendee.status),
                flag(attendee.rsvp),
                order(position),
                link(&attendee.icalendar_component_id, mode),
                index,
            ]
        })
        .collect()
}

fn alarm_rows(event: &CalendarImportEvent, mode: LinkMode) -> Vec<Vec<Field>> {
    event
        .alarms
        .iter()
        .enumerate()
        .map(|(position, alarm)| {
            vec![
                text(&alarm.id),
                text(&alarm.action),
                text(&alarm.trigger_type),
                text(&alarm.trigger_value),
                optional_text(alarm.description.as_deref()),
                order(position),
                link(&alarm.icalendar_component_id, mode),
            ]
        })
        .collect()
}

fn override_rows(event: &CalendarImportEvent, mode: LinkMode) -> Vec<Vec<Field>> {
    event
        .overrides
        .iter()
        .map(|row| {
            vec![
                text(&row.recurrence_id),
                text(&row.id),
                optional_text(row.recurrence_range.as_deref()),
                optional_text(row.title.as_deref()),
                optional_text(row.start_time.as_deref()),
                optional_text(row.end_time.as_deref()),
                sanitize_optional_calendar_description(&row.description)
                    .map_or(Field::Null, Field::Text),
                optional_text(row.location.as_deref()),
                optional_text(row.url.as_deref()),
                optional_integer(row.color),
                optional_text(row.status.as_deref()),
                optional_text(row.transparency.as_deref()),
                optional_text(row.visibility.as_deref()),
                link(&row.icalendar_component_id, mode),
                Field::Null,
                Field::Null,
            ]
        })
        .collect()
}

/// Extended property rows of every written override, under the override's vault id.
fn override_property_rows(
    event: &CalendarImportEvent,
    override_ids: &[(String, String)],
) -> Result<Vec<Vec<Field>>, String> {
    let ids: HashMap<&str, &str> = override_ids
        .iter()
        .map(|(payload, row)| (payload.as_str(), row.as_str()))
        .collect();
    let mut rows = Vec::new();
    for row in &event.overrides {
        let Some(override_id) = ids.get(row.id.as_str()) else {
            continue;
        };
        let properties =
            parse_string_map(&row.extended_properties, "override.extended_properties")?;
        for (position, (key, value)) in properties.into_iter().enumerate() {
            rows.push(vec![
                text(override_id),
                Field::Text(key),
                Field::Null,
                Field::Text(value),
                order(position),
            ]);
        }
    }
    Ok(rows)
}

/// Select list reading each column as its storage class and its value.
fn typed_columns(columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("typeof({column}), {column}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_fields(row: &SqliteRow, count: usize) -> Result<Vec<Field>, String> {
    (0..count)
        .map(|column| {
            let index = column * 2;
            let kind: String = row
                .try_get(index)
                .map_err(|e| format!("read stored column kind: {e}"))?;
            let field = match kind.as_str() {
                "null" => Field::Null,
                "integer" => Field::Integer(
                    row.try_get_unchecked(index + 1)
                        .map_err(|e| format!("read stored integer column: {e}"))?,
                ),
                "text" => Field::Text(
                    row.try_get_unchecked(index + 1)
                        .map_err(|e| format!("read stored text column: {e}"))?,
                ),
                "blob" => Field::Blob(
                    row.try_get_unchecked(index + 1)
                        .map_err(|e| format!("read stored blob column: {e}"))?,
                ),
                other => return Err(format!("stored column has unexpected type {other}")),
            };
            Ok(field)
        })
        .collect()
}

async fn stored_event(
    conn: &mut SqliteConnection,
    target_calendar_id: &str,
    event_id: &str,
) -> Result<Vec<Field>, String> {
    let sql = format!(
        "SELECT {} FROM calendar_events WHERE id = ? AND calendar_id = ?",
        typed_columns(EVENT_COLUMNS)
    );
    let row = sqlx::query(AssertSqlSafe(sql))
        .bind(event_id)
        .bind(target_calendar_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| format!("load imported event: {e}"))?
        .ok_or_else(|| format!("import target event '{event_id}' not found"))?;
    read_fields(&row, EVENT_COLUMNS.len())
}

async fn stored_rows(
    conn: &mut SqliteConnection,
    table: &ChildTable,
    owner_id: &str,
) -> Result<Vec<Vec<Field>>, String> {
    let sql = format!(
        "SELECT {} FROM {} WHERE {} = ? ORDER BY rowid",
        typed_columns(table.columns),
        table.table,
        table.owner
    );
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(owner_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| format!("load stored {}: {e}", table.table))?;
    rows.iter()
        .map(|row| read_fields(row, table.columns.len()))
        .collect()
}

async fn insert_event(
    conn: &mut SqliteConnection,
    target_calendar_id: &str,
    now: &str,
    event: &CalendarImportEvent,
    event_id: &str,
    row: Vec<Field>,
) -> Result<(), String> {
    let sql = format!(
        "INSERT INTO calendar_events (id, calendar_id, source_uid, created_at, updated_at, {})
         VALUES (?, ?, ?, ?, ?, {})",
        EVENT_COLUMNS.join(", "),
        vec!["?"; EVENT_COLUMNS.len()].join(", ")
    );
    let mut query = sqlx::query(AssertSqlSafe(sql))
        .bind(event_id)
        .bind(target_calendar_id)
        .bind(&event.source_uid)
        .bind(now)
        .bind(now);
    for field in row {
        query = bind_field(query, field);
    }
    query
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("insert imported event: {e}"))?;
    Ok(())
}

async fn update_event(
    conn: &mut SqliteConnection,
    target_calendar_id: &str,
    now: &str,
    event_id: &str,
    row: Vec<Field>,
) -> Result<(), String> {
    let assignments: Vec<String> = EVENT_COLUMNS
        .iter()
        .map(|column| format!("{column} = ?"))
        .collect();
    let sql = format!(
        "UPDATE calendar_events SET {}, updated_at = ? WHERE id = ? AND calendar_id = ?",
        assignments.join(", ")
    );
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for field in row {
        query = bind_field(query, field);
    }
    let result = query
        .bind(now)
        .bind(event_id)
        .bind(target_calendar_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("update imported event: {e}"))?;
    if result.rows_affected() == 0 {
        return Err(format!("import target event '{event_id}' not found"));
    }
    Ok(())
}

async fn delete_rows(
    conn: &mut SqliteConnection,
    diff: &TableDiff,
    owner_id: &str,
) -> Result<(), String> {
    if diff.deletes.is_empty() {
        return Ok(());
    }
    let table = diff.table;
    let conditions: Vec<String> = table
        .key
        .iter()
        .map(|&column| format!(" AND {} = ?", table.columns[column]))
        .collect();
    let sql = format!(
        "DELETE FROM {} WHERE {} = ?{}",
        table.table,
        table.owner,
        conditions.concat()
    );
    for key in &diff.deletes {
        let mut query = sqlx::query(AssertSqlSafe(sql.as_str())).bind(owner_id);
        for field in key {
            query = bind_field(query, field.clone());
        }
        query
            .execute(&mut *conn)
            .await
            .map_err(|e| format!("delete imported {}: {e}", table.table))?;
    }
    Ok(())
}

async fn insert_rows(
    conn: &mut SqliteConnection,
    diff: &TableDiff,
    owner_id: &str,
) -> Result<(), String> {
    if diff.inserts.is_empty() {
        return Ok(());
    }
    let table = diff.table;
    let values: Vec<&str> = (0..table.columns.len())
        .map(|column| {
            if Some(column) == table.id {
                "COALESCE(?, lower(hex(randomblob(16))))"
            } else {
                "?"
            }
        })
        .collect();
    let sql = format!(
        "INSERT INTO {} ({}, {}) VALUES (?, {})",
        table.table,
        table.owner,
        table.columns.join(", "),
        values.join(", ")
    );
    for row in &diff.inserts {
        let mut query = sqlx::query(AssertSqlSafe(sql.as_str())).bind(owner_id);
        for field in row {
            query = bind_field(query, field.clone());
        }
        query
            .execute(&mut *conn)
            .await
            .map_err(|e| format!("insert imported {}: {e}", table.table))?;
    }
    Ok(())
}
