use crate::data_sources;
use crate::data_sources::views::{
    ViewProperty, canonical_filter, canonical_sorts, generate_uuid_tx,
    load_active_data_source_and_database_tx, normalized_row_for_schema, parse_json, stored_filters,
    stored_sorts, view_schema,
};
use crate::models::{
    NoteDataSourceRow, NoteDataSourceRowWindow, NoteDataSourceTimelineConfigurationUpdate,
    NoteDataSourceTimelineViewDto, NoteDataSourceTimelineViewUpdate,
    NoteDataSourceViewWindowRequest, NoteDatabaseViewRow,
};
use chrono::NaiveDate;
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;

const DEFAULT_TIMELINE_VIEW_NAME: &str = "Timeline";
const MAX_TIMELINE_CONFIGURATION_BYTES: usize = 50 * 1024;
const TIMELINE_ROW_OPEN_MODES: &[&str] = &["full_page", "side_panel"];

#[cfg(test)]
pub async fn data_source_timeline_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceTimelineViewDto, String> {
    data_source_timeline_view_window(
        pool,
        data_source_id,
        database_id,
        view_id,
        NoteDataSourceViewWindowRequest::default(),
    )
    .await
}

pub async fn data_source_timeline_view_window(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window: NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceTimelineViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source timeline read: {e}"))?;
    let dto = load_timeline_view_tx(&mut tx, data_source_id, database_id, view_id, &window).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source timeline read: {e}"))?;
    Ok(dto)
}

pub async fn update_data_source_timeline_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    update: NoteDataSourceTimelineViewUpdate,
) -> Result<NoteDataSourceTimelineViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source timeline view update: {e}"))?;
    crate::project_history::mark_data_source_dirty_tx(
        &mut tx,
        data_source_id,
        "Timeline view",
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
    let filter = canonical_filter(&update.filter, &property_types, "timeline")?;
    let sorts = canonical_sorts(&update.sorts, &property_ids, "board")?;
    let configuration = canonical_timeline_configuration(&update.configuration, &schema)?;
    let view =
        ensure_timeline_view_row_tx(&mut tx, &data_source, database_id, view_id, &schema).await?;
    data_sources::views::prepare_view_mutation_tx(&mut tx, &view, "Timeline view").await?;
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
    .map_err(|e| format!("update notes data source timeline view: {e}"))?;
    let dto = load_timeline_view_tx(
        &mut tx,
        data_source_id,
        database_id,
        Some(&view.id),
        &NoteDataSourceViewWindowRequest::default(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source timeline view update: {e}"))?;
    Ok(dto)
}

async fn load_timeline_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window_request: &NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceTimelineViewDto, String> {
    let (data_source, database) =
        load_active_data_source_and_database_tx(tx, data_source_id, "board").await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = view_schema(&schema_properties)?;
    let view = ensure_timeline_view_row_tx(tx, &data_source, database_id, view_id, &schema).await?;
    let configuration = timeline_configuration(view.configuration.as_deref(), &schema)?;
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
    let raw_configuration = parse_json(
        view.configuration.as_deref().unwrap_or("{}"),
        "timeline view configuration",
    )?;
    let group_property = raw_configuration
        .get("timeline")
        .and_then(|value| value.get("group_property_id"))
        .and_then(Value::as_str)
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
                group_property,
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
    NoteDataSourceTimelineViewDto::new(data_source, database, view, window)
}

async fn ensure_timeline_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source: &NoteDataSourceRow,
    database_id: Option<&str>,
    view_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<NoteDatabaseViewRow, String> {
    if let Some(view) = load_timeline_view_row_tx(tx, &data_source.id, database_id, view_id).await?
    {
        return Ok(view);
    }
    let database_id = data_sources::views::scoped_database_id(data_source, database_id);
    crate::databases::editing_lock::ensure_unlocked_tx(tx, database_id).await?;
    let id = generate_uuid_tx(
        tx,
        "generate timeline view id",
        "generated_timeline_view_id",
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
         VALUES (?, ?, ?, ?, 'timeline', '[]', ?, ?)",
    )
    .bind(&id)
    .bind(database_id)
    .bind(&data_source.id)
    .bind(DEFAULT_TIMELINE_VIEW_NAME)
    .bind(default_timeline_configuration(schema, &range_start, &range_end).to_string())
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes timeline view: {e}"))?;
    load_timeline_view_row_tx(tx, &data_source.id, Some(database_id), Some(&id))
        .await?
        .ok_or_else(|| "inserted timeline view was not found".to_string())
}

async fn load_timeline_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<Option<NoteDatabaseViewRow>, String> {
    data_sources::views::load_scoped_view_row_tx(
        tx,
        data_source_id,
        "timeline",
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
    .map_err(|e| format!("prepare notes timeline default range: {e}"))
}

fn default_timeline_configuration(
    schema: &[ViewProperty],
    range_start: &str,
    range_end: &str,
) -> Value {
    let date_property_id = default_date_property_id(schema);
    let visible_property_ids =
        visible_timeline_property_ids(schema, date_property_id.as_deref(), None);
    json!({
        "type": "timeline",
        "timeline": {
            "date_property_id": date_property_id,
            "group_property_id": Value::Null,
            "group_order": [],
            "hidden_group_ids": [],
            "range_start": range_start,
            "range_end": range_end,
            "visible_property_ids": visible_property_ids,
            "row_open_mode": "side_panel"
        }
    })
}

fn canonical_timeline_configuration(
    update: &NoteDataSourceTimelineConfigurationUpdate,
    schema: &[ViewProperty],
) -> Result<Value, String> {
    let date_property_id = canonical_date_property_id(update.date_property_id.as_deref(), schema)?;
    let group_property_id =
        canonical_group_property_id(update.group_property_id.as_deref(), schema)?;
    let range = canonical_range(&update.range_start, &update.range_end)?;
    let visible_property_ids = canonical_visible_property_ids(
        &update.visible_property_ids,
        schema,
        date_property_id.as_deref(),
        group_property_id.as_deref(),
    );
    let row_open_mode = update.row_open_mode.trim();
    if !TIMELINE_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("timeline row_open_mode is not supported".to_string());
    }
    let value = json!({
        "type": "timeline",
        "timeline": {
            "date_property_id": date_property_id,
            "group_property_id": group_property_id,
            "group_order": unique_strings(&update.group_order),
            "hidden_group_ids": unique_strings(&update.hidden_group_ids),
            "range_start": range.start.to_string(),
            "range_end": range.end.to_string(),
            "visible_property_ids": visible_property_ids,
            "row_open_mode": row_open_mode
        }
    });
    if value.to_string().len() > MAX_TIMELINE_CONFIGURATION_BYTES {
        return Err("timeline configuration must not exceed 50KB".to_string());
    }
    Ok(value)
}

fn timeline_configuration(
    configuration: Option<&str>,
    schema: &[ViewProperty],
) -> Result<TimelineConfiguration, String> {
    let value = configuration
        .map(|configuration| parse_json(configuration, "timeline view configuration"))
        .transpose()?
        .ok_or_else(|| "timeline view configuration is required".to_string())?;
    let timeline = value
        .get("timeline")
        .and_then(Value::as_object)
        .ok_or_else(|| "timeline view configuration must contain timeline".to_string())?;
    let date_property_id = timeline
        .get("date_property_id")
        .and_then(Value::as_str)
        .filter(|id| is_date_property_id(id, schema))
        .map(str::to_string)
        .or_else(|| default_date_property_id(schema));
    if let Some(group_property_id) = timeline.get("group_property_id").and_then(Value::as_str) {
        validate_group_property_id(group_property_id, schema)?;
    }
    let range_start = timeline
        .get("range_start")
        .and_then(Value::as_str)
        .ok_or_else(|| "timeline range_start is required".to_string())?;
    let range_end = timeline
        .get("range_end")
        .and_then(Value::as_str)
        .ok_or_else(|| "timeline range_end is required".to_string())?;
    let range = canonical_range(range_start, range_end)?;
    let row_open_mode = timeline
        .get("row_open_mode")
        .and_then(Value::as_str)
        .unwrap_or("side_panel");
    if !TIMELINE_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("timeline row_open_mode is not supported".to_string());
    }
    Ok(TimelineConfiguration {
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
        .ok_or_else(|| "timeline date property references an unknown property".to_string())?;
    if property.property_type != "date" {
        return Err("timeline date property must be a date property".to_string());
    }
    Ok(Some(property_id.to_string()))
}

fn canonical_group_property_id(
    raw_property_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<Option<String>, String> {
    let Some(property_id) = raw_property_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    validate_group_property_id(property_id, schema)?;
    Ok(Some(property_id.to_string()))
}

fn validate_group_property_id(property_id: &str, schema: &[ViewProperty]) -> Result<(), String> {
    let property = schema
        .iter()
        .find(|property| property.id == property_id)
        .ok_or_else(|| "timeline group property references an unknown property".to_string())?;
    if !data_sources::views::GROUP_PROPERTY_TYPES.contains(&property.property_type.as_str()) {
        return Err("timeline group property type is not supported".to_string());
    }
    Ok(())
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

fn visible_timeline_property_ids(
    schema: &[ViewProperty],
    date_property_id: Option<&str>,
    group_property_id: Option<&str>,
) -> Vec<String> {
    schema
        .iter()
        .filter(|property| property.property_type != "title")
        .filter(|property| Some(property.id.as_str()) != date_property_id)
        .filter(|property| Some(property.id.as_str()) != group_property_id)
        .take(4)
        .map(|property| property.id.clone())
        .collect()
}

fn canonical_visible_property_ids(
    property_ids: &[String],
    schema: &[ViewProperty],
    date_property_id: Option<&str>,
    group_property_id: Option<&str>,
) -> Vec<String> {
    let known: HashSet<&str> = schema.iter().map(|property| property.id.as_str()).collect();
    let mut seen = HashSet::new();
    let mut visible = Vec::new();
    for property_id in property_ids {
        let property_id = property_id.trim();
        if property_id.is_empty()
            || property_id == "title"
            || Some(property_id) == date_property_id
            || Some(property_id) == group_property_id
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

fn canonical_range(range_start: &str, range_end: &str) -> Result<TimelineRange, String> {
    let start = parse_iso_date(range_start, "timeline range_start")?;
    let end = parse_iso_date(range_end, "timeline range_end")?;
    if end < start {
        return Err("timeline range_end must be on or after range_start".to_string());
    }
    Ok(TimelineRange { start, end })
}

fn parse_iso_date(value: &str, label: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| format!("{label} must be an ISO date"))
}

fn unique_strings(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || !seen.insert(value.to_string()) {
            continue;
        }
        unique.push(value.to_string());
    }
    unique
}

struct TimelineConfiguration {
    date_property_id: Option<String>,
    range: TimelineRange,
}

struct TimelineRange {
    start: NaiveDate,
    end: NaiveDate,
}
