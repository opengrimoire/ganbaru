use crate::data_sources;
use crate::data_sources::views::{
    ViewProperty, canonical_filter, canonical_sorts, generate_uuid_tx,
    load_active_data_source_and_database_tx, normalized_row_for_schema, parse_json, stored_filters,
    stored_sorts, view_schema,
};
use crate::models::{
    NoteDataSourceListConfigurationUpdate, NoteDataSourceListViewDto, NoteDataSourceListViewUpdate,
    NoteDataSourceRow, NoteDataSourceViewWindowRequest, NoteDatabaseViewRow,
};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;

const DEFAULT_LIST_VIEW_NAME: &str = "List";
const MAX_LIST_CONFIGURATION_BYTES: usize = 50 * 1024;
const LIST_ROW_OPEN_MODES: &[&str] = &["full_page", "side_panel"];

#[cfg(test)]
pub async fn data_source_list_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceListViewDto, String> {
    data_source_list_view_window(
        pool,
        data_source_id,
        database_id,
        view_id,
        NoteDataSourceViewWindowRequest::default(),
    )
    .await
}

pub async fn data_source_list_view_window(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window: NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceListViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source list read: {e}"))?;
    let dto = load_list_view_tx(&mut tx, data_source_id, database_id, view_id, &window).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source list read: {e}"))?;
    Ok(dto)
}

pub async fn update_data_source_list_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    update: NoteDataSourceListViewUpdate,
) -> Result<NoteDataSourceListViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source list view update: {e}"))?;
    crate::project_history::mark_data_source_dirty_tx(&mut tx, data_source_id, "List view", false)
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
    let filter = canonical_filter(&update.filter, &property_types, "list")?;
    let sorts = canonical_sorts(&update.sorts, &property_ids, "board")?;
    let configuration = canonical_list_configuration(&update.configuration, &schema)?;
    let view =
        ensure_list_view_row_tx(&mut tx, &data_source, database_id, view_id, &schema).await?;
    data_sources::views::prepare_view_mutation_tx(&mut tx, &view, "List view").await?;
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
    .map_err(|e| format!("update notes data source list view: {e}"))?;
    let dto = load_list_view_tx(
        &mut tx,
        data_source_id,
        database_id,
        Some(&view.id),
        &NoteDataSourceViewWindowRequest::default(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source list view update: {e}"))?;
    Ok(dto)
}

async fn load_list_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window_request: &NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceListViewDto, String> {
    let (data_source, database) =
        load_active_data_source_and_database_tx(tx, data_source_id, "board").await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = view_schema(&schema_properties)?;
    let view = ensure_list_view_row_tx(tx, &data_source, database_id, view_id, &schema).await?;
    validate_list_configuration(view.configuration.as_deref(), &schema)?;
    let filters = stored_filters(view.filter.as_deref(), "database board filter", "board")?;
    let sorts = stored_sorts(&view.sorts, "database board sorts", "board")?;
    let window_schema = data_sources::window::table_properties_from_view(&schema);
    let configuration = parse_json(
        view.configuration.as_deref().unwrap_or("{}"),
        "list view configuration",
    )?;
    let group_property = configuration
        .get("list")
        .and_then(|value| value.get("group_property_id"))
        .and_then(Value::as_str)
        .and_then(|id| window_schema.iter().find(|property| property.id == id));
    let mut window = data_sources::window::load_row_window_tx(
        tx,
        data_source_id,
        data_sources::window::RowWindowQuery {
            schema: &window_schema,
            filters: &filters,
            sorts: &sorts,
            request: window_request,
            date_property: None,
            group_property,
        },
    )
    .await?;
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
    NoteDataSourceListViewDto::new(data_source, database, view, window)
}

async fn ensure_list_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source: &NoteDataSourceRow,
    database_id: Option<&str>,
    view_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<NoteDatabaseViewRow, String> {
    if let Some(view) = load_list_view_row_tx(tx, &data_source.id, database_id, view_id).await? {
        return Ok(view);
    }
    let database_id = data_sources::views::scoped_database_id(data_source, database_id);
    crate::databases::editing_lock::ensure_unlocked_tx(tx, database_id).await?;
    let id = generate_uuid_tx(tx, "generate list view id", "generated_list_view_id").await?;
    let sort_order = data_sources::views::next_view_sort_order_tx(tx, database_id).await?;
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
         VALUES (?, ?, ?, ?, 'list', '[]', ?, ?)",
    )
    .bind(&id)
    .bind(database_id)
    .bind(&data_source.id)
    .bind(DEFAULT_LIST_VIEW_NAME)
    .bind(default_list_configuration(schema).to_string())
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes list view: {e}"))?;
    load_list_view_row_tx(tx, &data_source.id, Some(database_id), Some(&id))
        .await?
        .ok_or_else(|| "inserted list view was not found".to_string())
}

async fn load_list_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<Option<NoteDatabaseViewRow>, String> {
    data_sources::views::load_scoped_view_row_tx(tx, data_source_id, "list", database_id, view_id)
        .await
}

fn default_list_configuration(schema: &[ViewProperty]) -> Value {
    json!({
        "type": "list",
        "list": {
            "group_property_id": null,
            "group_order": [],
            "hidden_group_ids": [],
            "visible_property_ids": visible_list_property_ids(schema, None),
            "row_open_mode": "side_panel"
        }
    })
}

fn canonical_list_configuration(
    update: &NoteDataSourceListConfigurationUpdate,
    schema: &[ViewProperty],
) -> Result<Value, String> {
    let group_property_id =
        canonical_group_property_id(update.group_property_id.as_deref(), schema)?;
    let visible_property_ids = canonical_visible_property_ids(
        &update.visible_property_ids,
        schema,
        group_property_id.as_deref(),
    );
    let row_open_mode = update.row_open_mode.trim();
    if !LIST_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("list row_open_mode is not supported".to_string());
    }
    let value = json!({
        "type": "list",
        "list": {
            "group_property_id": group_property_id,
            "group_order": unique_strings(&update.group_order),
            "hidden_group_ids": unique_strings(&update.hidden_group_ids),
            "visible_property_ids": visible_property_ids,
            "row_open_mode": row_open_mode
        }
    });
    if value.to_string().len() > MAX_LIST_CONFIGURATION_BYTES {
        return Err("list configuration must not exceed 50KB".to_string());
    }
    Ok(value)
}

fn validate_list_configuration(
    configuration: Option<&str>,
    schema: &[ViewProperty],
) -> Result<(), String> {
    let value = configuration
        .map(|configuration| parse_json(configuration, "list view configuration"))
        .transpose()?
        .unwrap_or_else(|| default_list_configuration(schema));
    let list = value
        .get("list")
        .and_then(Value::as_object)
        .ok_or_else(|| "list view configuration must contain list".to_string())?;
    canonical_group_property_id(
        list.get("group_property_id").and_then(Value::as_str),
        schema,
    )?;
    let row_open_mode = list
        .get("row_open_mode")
        .and_then(Value::as_str)
        .unwrap_or("side_panel");
    if !LIST_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("list row_open_mode is not supported".to_string());
    }
    Ok(())
}

fn canonical_group_property_id(
    raw_property_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<Option<String>, String> {
    let Some(property_id) = raw_property_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let property = schema
        .iter()
        .find(|property| property.id == property_id)
        .ok_or_else(|| "list group property references an unknown property".to_string())?;
    if !data_sources::views::GROUP_PROPERTY_TYPES.contains(&property.property_type.as_str()) {
        return Err("list group property type is not supported".to_string());
    }
    Ok(Some(property_id.to_string()))
}

fn visible_list_property_ids(
    schema: &[ViewProperty],
    group_property_id: Option<&str>,
) -> Vec<String> {
    schema
        .iter()
        .filter(|property| property.property_type != "title")
        .filter(|property| Some(property.id.as_str()) != group_property_id)
        .take(4)
        .map(|property| property.id.clone())
        .collect()
}

fn canonical_visible_property_ids(
    property_ids: &[String],
    schema: &[ViewProperty],
    group_property_id: Option<&str>,
) -> Vec<String> {
    let known: HashSet<&str> = schema.iter().map(|property| property.id.as_str()).collect();
    let mut seen = HashSet::new();
    let mut visible = Vec::new();
    for property_id in property_ids {
        let property_id = property_id.trim();
        if property_id.is_empty()
            || property_id == "title"
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
