use crate::data_sources;
use crate::data_sources::views::{
    ViewProperty, canonical_filter, canonical_sorts, generate_uuid_tx,
    load_active_data_source_and_database_tx, normalized_row_for_schema, parse_json, stored_filters,
    stored_sorts, view_schema,
};
use crate::models::{
    NoteDataSourceCalendarConfigurationUpdate, NoteDataSourceCalendarViewDto,
    NoteDataSourceCalendarViewUpdate, NoteDataSourceRow, NoteDataSourceRowWindow,
    NoteDataSourceViewWindowRequest, NoteDatabaseViewRow,
};
use chrono::NaiveDate;
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;

const DEFAULT_CALENDAR_VIEW_NAME: &str = "Calendar";
const MAX_CALENDAR_CONFIGURATION_BYTES: usize = 50 * 1024;
const CALENDAR_ROW_OPEN_MODES: &[&str] = &["full_page", "side_panel"];

#[cfg(test)]
pub async fn data_source_calendar_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceCalendarViewDto, String> {
    data_source_calendar_view_window(
        pool,
        data_source_id,
        database_id,
        view_id,
        NoteDataSourceViewWindowRequest::default(),
    )
    .await
}

pub async fn data_source_calendar_view_window(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window: NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceCalendarViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source calendar read: {e}"))?;
    let dto = load_calendar_view_tx(&mut tx, data_source_id, database_id, view_id, &window).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source calendar read: {e}"))?;
    Ok(dto)
}

pub async fn update_data_source_calendar_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    update: NoteDataSourceCalendarViewUpdate,
) -> Result<NoteDataSourceCalendarViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source calendar view update: {e}"))?;
    crate::project_history::mark_data_source_dirty_tx(
        &mut tx,
        data_source_id,
        "Calendar view",
        false,
    )
    .await?;
    let (data_source, _database) =
        load_active_data_source_and_database_tx(&mut tx, data_source_id, "board").await?;
    let schema = view_schema(&parse_json(
        &data_source.properties,
        "data source properties",
    )?)?;
    let property_ids: HashSet<String> = schema.iter().map(|property| property.id.clone()).collect();
    let property_types = schema
        .iter()
        .map(|property| (property.id.as_str(), property.property_type.as_str()))
        .collect();
    let filter = canonical_filter(&update.filter, &property_types, "calendar")?;
    let sorts = canonical_sorts(&update.sorts, &property_ids, "board")?;
    let configuration = canonical_calendar_configuration(&update.configuration, &schema)?;
    let view =
        ensure_calendar_view_row_tx(&mut tx, &data_source, database_id, view_id, &schema).await?;
    data_sources::views::prepare_view_mutation_tx(&mut tx, &view, "Calendar view").await?;
    sqlx::query(
        "UPDATE notes_database_views
         SET filter = ?,
             sorts = ?,
             configuration = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(filter.map(|value| value.to_string()))
    .bind(sorts.to_string())
    .bind(configuration.to_string())
    .bind(&view.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes data source calendar view: {e}"))?;
    let dto = load_calendar_view_tx(
        &mut tx,
        data_source_id,
        database_id,
        Some(&view.id),
        &NoteDataSourceViewWindowRequest::default(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source calendar view update: {e}"))?;
    Ok(dto)
}

async fn load_calendar_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window_request: &NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceCalendarViewDto, String> {
    let (data_source, database) =
        load_active_data_source_and_database_tx(tx, data_source_id, "board").await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = view_schema(&schema_properties)?;
    let view = ensure_calendar_view_row_tx(tx, &data_source, database_id, view_id, &schema).await?;
    let configuration = calendar_configuration(view.configuration.as_deref(), &schema)?;
    let filters = stored_filters(view.filter.as_deref(), "database board filter", "board")?;
    let sorts = stored_sorts(&view.sorts, "database board sorts", "board")?;
    let window_schema = data_sources::window::table_properties_from_view(&schema);
    let mut effective_window = window_request.clone();
    effective_window
        .range_start
        .get_or_insert_with(|| configuration.range.start.to_string());
    effective_window
        .range_end
        .get_or_insert_with(|| configuration.range.end.to_string());
    let date_property = configuration
        .date_property_id
        .as_deref()
        .and_then(|id| window_schema.iter().find(|property| property.id == id));
    let mut window = if date_property.is_some() {
        data_sources::window::load_row_window_tx(
            tx,
            data_source_id,
            data_sources::window::RowWindowQuery {
                schema: &window_schema,
                filters: &filters,
                sorts: &sorts,
                request: &effective_window,
                date_property,
                group_property: None,
            },
        )
        .await?
    } else {
        NoteDataSourceRowWindow {
            rows: Vec::new(),
            total_row_count: 0,
            next_cursor: None,
            has_more: false,
            group_counts: std::collections::HashMap::new(),
        }
    };
    window.rows = window
        .rows
        .into_iter()
        .map(|row| normalized_row_for_schema(row, &schema))
        .collect::<Result<Vec<_>, _>>()?;
    data_sources::relations::hydrate_relation_titles_tx(tx, &mut window.rows).await?;
    data_sources::rollups::hydrate_rollups_tx(
        tx,
        data_source_id,
        &schema_properties,
        &mut window.rows,
    )
    .await?;
    data_sources::formulas::hydrate_formulas(&schema_properties, &mut window.rows)?;
    data_sources::buttons::hydrate_buttons(&schema_properties, &mut window.rows)?;
    NoteDataSourceCalendarViewDto::new(data_source, database, view, window)
}

async fn ensure_calendar_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source: &NoteDataSourceRow,
    database_id: Option<&str>,
    view_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<NoteDatabaseViewRow, String> {
    if let Some(view) = load_calendar_view_row_tx(tx, &data_source.id, database_id, view_id).await?
    {
        return Ok(view);
    }
    let database_id = data_sources::views::scoped_database_id(data_source, database_id);
    crate::databases::editing_lock::ensure_unlocked_tx(tx, database_id).await?;
    let id = generate_uuid_tx(
        tx,
        "generate calendar view id",
        "generated_calendar_view_id",
    )
    .await?;
    let sort_order = data_sources::views::next_view_sort_order_tx(tx, database_id).await?;
    let (range_start, range_end) = current_month_range_tx(tx).await?;
    sqlx::query(
        "INSERT INTO notes_database_views (
            id,
            database_id,
            data_source_id,
            name,
            type,
            sorts,
            configuration,
            sort_order
         )
         VALUES (?, ?, ?, ?, 'calendar', '[]', ?, ?)",
    )
    .bind(&id)
    .bind(database_id)
    .bind(&data_source.id)
    .bind(DEFAULT_CALENDAR_VIEW_NAME)
    .bind(default_calendar_configuration(schema, &range_start, &range_end).to_string())
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes calendar view: {e}"))?;
    load_calendar_view_row_tx(tx, &data_source.id, Some(database_id), Some(&id))
        .await?
        .ok_or_else(|| "inserted calendar view was not found".to_string())
}

async fn load_calendar_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<Option<NoteDatabaseViewRow>, String> {
    data_sources::views::load_scoped_view_row_tx(
        tx,
        data_source_id,
        "calendar",
        database_id,
        view_id,
    )
    .await
}

async fn current_month_range_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(String, String), String> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT date('now', 'start of month'), date('now', 'start of month', '+1 month', '-1 day')",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("prepare notes calendar default range: {e}"))
}

fn default_calendar_configuration(
    schema: &[ViewProperty],
    range_start: &str,
    range_end: &str,
) -> Value {
    let date_property_id = default_date_property_id(schema);
    let visible_property_ids = visible_calendar_property_ids(schema, date_property_id.as_deref());
    json!({
        "type": "calendar",
        "calendar": {
            "date_property_id": date_property_id,
            "range_start": range_start,
            "range_end": range_end,
            "visible_property_ids": visible_property_ids,
            "row_open_mode": "side_panel"
        }
    })
}

fn canonical_calendar_configuration(
    update: &NoteDataSourceCalendarConfigurationUpdate,
    schema: &[ViewProperty],
) -> Result<Value, String> {
    let date_property_id = canonical_date_property_id(update.date_property_id.as_deref(), schema)?;
    let range = canonical_range(&update.range_start, &update.range_end)?;
    let visible_property_ids = canonical_visible_property_ids(
        &update.visible_property_ids,
        schema,
        date_property_id.as_deref(),
    );
    let row_open_mode = update.row_open_mode.trim();
    if !CALENDAR_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("calendar row_open_mode is not supported".to_string());
    }
    let value = json!({
        "type": "calendar",
        "calendar": {
            "date_property_id": date_property_id,
            "range_start": range.start.to_string(),
            "range_end": range.end.to_string(),
            "visible_property_ids": visible_property_ids,
            "row_open_mode": row_open_mode
        }
    });
    if value.to_string().len() > MAX_CALENDAR_CONFIGURATION_BYTES {
        return Err("calendar configuration must not exceed 50KB".to_string());
    }
    Ok(value)
}

fn calendar_configuration(
    configuration: Option<&str>,
    schema: &[ViewProperty],
) -> Result<CalendarConfiguration, String> {
    let value = configuration
        .map(|configuration| parse_json(configuration, "calendar view configuration"))
        .transpose()?
        .ok_or_else(|| "calendar view configuration is required".to_string())?;
    let calendar = value
        .get("calendar")
        .and_then(Value::as_object)
        .ok_or_else(|| "calendar view configuration must contain calendar".to_string())?;
    let date_property_id = calendar
        .get("date_property_id")
        .and_then(Value::as_str)
        .filter(|id| is_date_property_id(id, schema))
        .map(str::to_string)
        .or_else(|| default_date_property_id(schema));
    let range_start = calendar
        .get("range_start")
        .and_then(Value::as_str)
        .ok_or_else(|| "calendar range_start is required".to_string())?;
    let range_end = calendar
        .get("range_end")
        .and_then(Value::as_str)
        .ok_or_else(|| "calendar range_end is required".to_string())?;
    let range = canonical_range(range_start, range_end)?;
    let row_open_mode = calendar
        .get("row_open_mode")
        .and_then(Value::as_str)
        .unwrap_or("side_panel");
    if !CALENDAR_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("calendar row_open_mode is not supported".to_string());
    }
    Ok(CalendarConfiguration {
        date_property_id,
        range,
    })
}

fn canonical_date_property_id(
    raw_property_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<Option<String>, String> {
    let Some(property_id) = raw_property_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let property = schema
        .iter()
        .find(|property| property.id == property_id)
        .ok_or_else(|| "calendar date property references an unknown property".to_string())?;
    if property.property_type != "date" {
        return Err("calendar date property must be a date property".to_string());
    }
    Ok(Some(property_id.to_string()))
}

fn default_date_property_id(schema: &[ViewProperty]) -> Option<String> {
    schema
        .iter()
        .find(|property| property.property_type == "date")
        .map(|property| property.id.clone())
}

fn is_date_property_id(property_id: &str, schema: &[ViewProperty]) -> bool {
    schema
        .iter()
        .any(|property| property.id == property_id && property.property_type == "date")
}

fn visible_calendar_property_ids(
    schema: &[ViewProperty],
    date_property_id: Option<&str>,
) -> Vec<String> {
    schema
        .iter()
        .filter(|property| property.property_type != "title")
        .filter(|property| Some(property.id.as_str()) != date_property_id)
        .take(4)
        .map(|property| property.id.clone())
        .collect()
}

fn canonical_visible_property_ids(
    property_ids: &[String],
    schema: &[ViewProperty],
    date_property_id: Option<&str>,
) -> Vec<String> {
    let known: HashSet<&str> = schema.iter().map(|property| property.id.as_str()).collect();
    let mut seen = HashSet::new();
    let mut visible = Vec::new();
    for property_id in property_ids {
        let property_id = property_id.trim();
        if property_id.is_empty()
            || property_id == "title"
            || Some(property_id) == date_property_id
            || !known.contains(property_id)
        {
            continue;
        }
        if seen.insert(property_id.to_string()) {
            visible.push(property_id.to_string());
        }
    }
    visible
}

fn canonical_range(range_start: &str, range_end: &str) -> Result<CalendarRange, String> {
    let start = parse_iso_date(range_start, "calendar range_start")?;
    let end = parse_iso_date(range_end, "calendar range_end")?;
    if end < start {
        return Err("calendar range_end must be on or after range_start".to_string());
    }
    Ok(CalendarRange { start, end })
}

fn parse_iso_date(value: &str, label: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| format!("{label} must be an ISO date"))
}

struct CalendarConfiguration {
    date_property_id: Option<String>,
    range: CalendarRange,
}

struct CalendarRange {
    start: NaiveDate,
    end: NaiveDate,
}
