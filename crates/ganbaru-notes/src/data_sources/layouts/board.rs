use crate::models::{
    NoteDataSourceBoardConfigurationUpdate, NoteDataSourceBoardGroupDto,
    NoteDataSourceBoardRowMove, NoteDataSourceBoardViewDto, NoteDataSourceBoardViewUpdate,
    NoteDataSourceRow, NoteDataSourceRowPropertyUpdate, NoteDataSourceViewWindowRequest,
    NoteDatabaseViewRow, NotePageRow,
};
use crate::validation::require_uuid;
use crate::{
    data_sources,
    data_sources::views::{
        ViewProperty as BoardProperty, canonical_filter, canonical_sorts, generate_uuid_tx,
        load_active_data_source_and_database_tx, normalized_row_for_schema, parse_json,
        stored_filters, stored_sorts, view_schema as board_schema,
    },
};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, HashSet};

const DEFAULT_BOARD_VIEW_NAME: &str = "Board";
const MAX_BOARD_CONFIGURATION_BYTES: usize = 50 * 1024;
const BOARD_EMPTY_GROUP_ID: &str = "__empty__";
const BOARD_UNGROUPED_ID: &str = "__ungrouped__";
const BOARD_ROW_OPEN_MODES: &[&str] = &["full_page", "side_panel"];

pub async fn data_source_board_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceBoardViewDto, String> {
    data_source_board_view_window(
        pool,
        data_source_id,
        database_id,
        view_id,
        NoteDataSourceViewWindowRequest::default(),
    )
    .await
}

pub async fn data_source_board_view_window(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window: NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceBoardViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source board read: {e}"))?;
    let dto = load_board_view_tx(&mut tx, data_source_id, database_id, view_id, &window).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source board read: {e}"))?;
    Ok(dto)
}

pub async fn update_data_source_board_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    update: NoteDataSourceBoardViewUpdate,
) -> Result<NoteDataSourceBoardViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source board view update: {e}"))?;
    crate::project_history::mark_data_source_dirty_tx(&mut tx, data_source_id, "Board view", false)
        .await?;
    let (data_source, _database) =
        load_active_data_source_and_database_tx(&mut tx, data_source_id, "board").await?;
    let schema = board_schema(&parse_json(
        &data_source.properties,
        "data source properties",
    )?)?;
    let property_ids: HashSet<String> = schema.iter().map(|property| property.id.clone()).collect();
    let property_types = schema
        .iter()
        .map(|property| (property.id.as_str(), property.property_type.as_str()))
        .collect();
    let filter = canonical_filter(&update.filter, &property_types, "board")?;
    let sorts = canonical_sorts(&update.sorts, &property_ids, "board")?;
    let configuration = canonical_board_configuration(&update.configuration, &schema)?;
    let view =
        ensure_board_view_row_tx(&mut tx, &data_source, database_id, view_id, &schema).await?;
    data_sources::views::prepare_view_mutation_tx(&mut tx, &view, "Board view").await?;
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
    .map_err(|e| format!("update notes data source board view: {e}"))?;
    let dto = load_board_view_tx(
        &mut tx,
        data_source_id,
        database_id,
        Some(&view.id),
        &NoteDataSourceViewWindowRequest::default(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source board view update: {e}"))?;
    Ok(dto)
}

pub async fn move_data_source_board_row(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    request: NoteDataSourceBoardRowMove,
) -> Result<NoteDataSourceBoardViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    require_uuid(&request.page_id, "page_id")?;
    let group_id = request.group_id.trim();
    if group_id.is_empty() {
        return Err("board group_id is required".to_string());
    }
    let group_property = {
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| format!("begin notes data source board row move: {e}"))?;
        let (data_source, _) =
            load_active_data_source_and_database_tx(&mut tx, data_source_id, "board").await?;
        let schema = board_schema(&parse_json(
            &data_source.properties,
            "data source properties",
        )?)?;
        let view =
            ensure_board_view_row_tx(&mut tx, &data_source, database_id, view_id, &schema).await?;
        let configuration = board_configuration(view.configuration.as_deref(), &schema)?;
        let group_property_id = configuration
            .group_property_id
            .as_deref()
            .ok_or_else(|| "board view has no group property".to_string())?;
        let property = schema
            .iter()
            .find(|property| property.id == group_property_id)
            .cloned()
            .ok_or_else(|| "board group property was not found".to_string())?;
        tx.commit()
            .await
            .map_err(|e| format!("commit notes data source board row move read: {e}"))?;
        property
    };
    let value = board_move_value(&group_property, group_id)?;
    data_sources::layouts::table::update_data_source_row_property(
        pool,
        data_source_id,
        &request.page_id,
        NoteDataSourceRowPropertyUpdate {
            property_id: group_property.id,
            value,
        },
    )
    .await?;
    data_source_board_view(pool, data_source_id, database_id, view_id).await
}

async fn load_board_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window_request: &NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceBoardViewDto, String> {
    let (data_source, database) =
        load_active_data_source_and_database_tx(tx, data_source_id, "board").await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = board_schema(&schema_properties)?;
    let view = ensure_board_view_row_tx(tx, &data_source, database_id, view_id, &schema).await?;
    let configuration = board_configuration(view.configuration.as_deref(), &schema)?;
    let filters = stored_filters(view.filter.as_deref(), "database board filter", "board")?;
    let sorts = stored_sorts(&view.sorts, "database board sorts", "board")?;
    let window_schema = data_sources::window::table_properties_from_view(&schema);
    let group_property = configuration
        .group_property_id
        .as_deref()
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
    let groups = board_groups(&schema, &configuration, std::mem::take(&mut window.rows))?;
    NoteDataSourceBoardViewDto::new(data_source, database, view, groups, window)
}

async fn ensure_board_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source: &NoteDataSourceRow,
    database_id: Option<&str>,
    view_id: Option<&str>,
    schema: &[BoardProperty],
) -> Result<NoteDatabaseViewRow, String> {
    if let Some(view) = load_board_view_row_tx(tx, &data_source.id, database_id, view_id).await? {
        return Ok(view);
    }
    let database_id = data_sources::views::scoped_database_id(data_source, database_id);
    crate::databases::editing_lock::ensure_unlocked_tx(tx, database_id).await?;
    let id = generate_uuid_tx(tx, "generate board view id", "generated_board_view_id").await?;
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
         VALUES (?, ?, ?, ?, 'board', '[]', ?, ?)",
    )
    .bind(&id)
    .bind(database_id)
    .bind(&data_source.id)
    .bind(DEFAULT_BOARD_VIEW_NAME)
    .bind(default_board_configuration(schema).to_string())
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes board view: {e}"))?;
    load_board_view_row_tx(tx, &data_source.id, Some(database_id), Some(&id))
        .await?
        .ok_or_else(|| "inserted board view was not found".to_string())
}

async fn load_board_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<Option<NoteDatabaseViewRow>, String> {
    data_sources::views::load_scoped_view_row_tx(tx, data_source_id, "board", database_id, view_id)
        .await
}

#[derive(Clone)]
struct BoardConfiguration {
    group_property_id: Option<String>,
    group_order: Vec<String>,
    hidden_group_ids: HashSet<String>,
}

struct BoardGroupDraft {
    id: String,
    name: String,
    color: String,
    hidden: bool,
    rows: Vec<NotePageRow>,
}

fn default_board_configuration(schema: &[BoardProperty]) -> Value {
    let group_property_id = default_group_property_id(schema);
    json!({
        "type": "board",
        "board": {
            "group_property_id": group_property_id,
            "group_order": [],
            "hidden_group_ids": [],
            "visible_property_ids": visible_board_property_ids(schema, group_property_id.as_deref()),
            "row_open_mode": "full_page"
        }
    })
}

fn canonical_board_configuration(
    update: &NoteDataSourceBoardConfigurationUpdate,
    schema: &[BoardProperty],
) -> Result<Value, String> {
    let property_ids: HashSet<&str> = schema.iter().map(|property| property.id.as_str()).collect();
    let group_property_id = match update.group_property_id.as_deref().map(str::trim) {
        Some("") | None => None,
        Some(id) => {
            let property = schema
                .iter()
                .find(|property| property.id == id)
                .ok_or_else(|| "board group property references an unknown property".to_string())?;
            if !data_sources::views::GROUP_PROPERTY_TYPES.contains(&property.property_type.as_str())
            {
                return Err("board group property type is not supported".to_string());
            }
            Some(id.to_string())
        }
    };
    let group_order = unique_strings(&update.group_order);
    let hidden_group_ids = unique_strings(&update.hidden_group_ids);
    let mut visible_property_ids = Vec::new();
    let mut seen = HashSet::new();
    for id in &update.visible_property_ids {
        let id = id.trim();
        if id.is_empty() || id == "title" || !property_ids.contains(id) {
            continue;
        }
        if seen.insert(id.to_string()) {
            visible_property_ids.push(id.to_string());
        }
    }
    let row_open_mode = update.row_open_mode.trim();
    if !BOARD_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("board row_open_mode is not supported".to_string());
    }
    let value = json!({
        "type": "board",
        "board": {
            "group_property_id": group_property_id,
            "group_order": group_order,
            "hidden_group_ids": hidden_group_ids,
            "visible_property_ids": visible_property_ids,
            "row_open_mode": row_open_mode
        }
    });
    if value.to_string().len() > MAX_BOARD_CONFIGURATION_BYTES {
        return Err("board configuration must not exceed 50KB".to_string());
    }
    Ok(value)
}

fn board_configuration(
    configuration: Option<&str>,
    schema: &[BoardProperty],
) -> Result<BoardConfiguration, String> {
    let value = configuration
        .map(|configuration| parse_json(configuration, "board view configuration"))
        .transpose()?
        .unwrap_or_else(|| default_board_configuration(schema));
    let board = value
        .get("board")
        .and_then(Value::as_object)
        .ok_or_else(|| "board view configuration must contain board".to_string())?;
    let group_property_id = board
        .get("group_property_id")
        .and_then(Value::as_str)
        .filter(|id| schema.iter().any(|property| property.id == *id))
        .map(str::to_string)
        .or_else(|| default_group_property_id(schema));
    let group_order = string_array(board.get("group_order")).unwrap_or_default();
    let hidden_group_ids = string_array(board.get("hidden_group_ids"))
        .unwrap_or_default()
        .into_iter()
        .collect();
    Ok(BoardConfiguration {
        group_property_id,
        group_order,
        hidden_group_ids,
    })
}

fn default_group_property_id(schema: &[BoardProperty]) -> Option<String> {
    data_sources::views::GROUP_PROPERTY_TYPES
        .iter()
        .find_map(|property_type| {
            schema
                .iter()
                .find(|property| property.property_type == *property_type)
                .map(|property| property.id.clone())
        })
}

fn visible_board_property_ids(
    schema: &[BoardProperty],
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

fn board_groups(
    schema: &[BoardProperty],
    configuration: &BoardConfiguration,
    rows: Vec<NotePageRow>,
) -> Result<Vec<NoteDataSourceBoardGroupDto>, String> {
    let group_property = configuration
        .group_property_id
        .as_deref()
        .and_then(|id| schema.iter().find(|property| property.id == id));
    let mut groups = initial_groups(group_property, configuration);
    for row in rows {
        let group_ids = group_property
            .map(|property| row_group_ids(&row, property))
            .unwrap_or_else(|| vec![BOARD_UNGROUPED_ID.to_string()]);
        for group_id in group_ids {
            if !groups.contains_key(&group_id) {
                let (name, color) = dynamic_group_label(&group_id, group_property);
                groups.insert(
                    group_id.clone(),
                    BoardGroupDraft {
                        id: group_id.clone(),
                        name,
                        color,
                        hidden: false,
                        rows: Vec::new(),
                    },
                );
            }
            if let Some(group) = groups.get_mut(&group_id) {
                group.rows.push(row.clone());
            }
        }
    }
    let mut ordered = Vec::new();
    for group_id in &configuration.group_order {
        if let Some(group) = groups.remove(group_id) {
            ordered.push(group);
        }
    }
    ordered.extend(groups.into_values());
    ordered
        .into_iter()
        .map(|group| {
            NoteDataSourceBoardGroupDto::new(
                group.id,
                group.name,
                group.color,
                group.hidden,
                group.rows,
            )
        })
        .collect()
}

fn initial_groups(
    group_property: Option<&BoardProperty>,
    configuration: &BoardConfiguration,
) -> BTreeMap<String, BoardGroupDraft> {
    let mut groups = BTreeMap::new();
    let Some(property) = group_property else {
        groups.insert(
            BOARD_UNGROUPED_ID.to_string(),
            group_draft(BOARD_UNGROUPED_ID, "Ungrouped", "default", configuration),
        );
        return groups;
    };
    match property.property_type.as_str() {
        "select" | "multi_select" | "status" => {
            for option in property_options(property) {
                groups.insert(
                    option.id.clone(),
                    group_draft(&option.id, &option.name, &option.color, configuration),
                );
            }
            groups.insert(
                BOARD_EMPTY_GROUP_ID.to_string(),
                group_draft(BOARD_EMPTY_GROUP_ID, "No value", "default", configuration),
            );
        }
        "checkbox" => {
            groups.insert(
                "false".to_string(),
                group_draft("false", "Unchecked", "gray", configuration),
            );
            groups.insert(
                "true".to_string(),
                group_draft("true", "Checked", "green", configuration),
            );
        }
        "date" | "people" | "relation" => {
            groups.insert(
                BOARD_EMPTY_GROUP_ID.to_string(),
                group_draft(BOARD_EMPTY_GROUP_ID, "No value", "default", configuration),
            );
        }
        _ => {
            groups.insert(
                BOARD_UNGROUPED_ID.to_string(),
                group_draft(BOARD_UNGROUPED_ID, "Ungrouped", "default", configuration),
            );
        }
    }
    groups
}

fn group_draft(
    id: &str,
    name: &str,
    color: &str,
    configuration: &BoardConfiguration,
) -> BoardGroupDraft {
    BoardGroupDraft {
        id: id.to_string(),
        name: name.to_string(),
        color: color.to_string(),
        hidden: configuration.hidden_group_ids.contains(id),
        rows: Vec::new(),
    }
}

struct BoardOption {
    id: String,
    name: String,
    color: String,
}

fn property_options(property: &BoardProperty) -> Vec<BoardOption> {
    property
        .schema
        .get(&property.property_type)
        .and_then(|config| config.get("options"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|option| {
            Some(BoardOption {
                id: option.get("id")?.as_str()?.to_string(),
                name: option.get("name")?.as_str()?.to_string(),
                color: option
                    .get("color")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string(),
            })
        })
        .collect()
}

fn row_group_ids(row: &NotePageRow, property: &BoardProperty) -> Vec<String> {
    match property.property_type.as_str() {
        "select" | "status" => row_property_payload(row, property)
            .and_then(|payload| {
                payload
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .filter(|id| !id.is_empty())
            .map(|id| vec![id])
            .unwrap_or_else(|| vec![BOARD_EMPTY_GROUP_ID.to_string()]),
        "multi_select" => {
            let ids: Vec<String> = row_property_payload(row, property)
                .and_then(|payload| payload.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                .collect();
            if ids.is_empty() {
                vec![BOARD_EMPTY_GROUP_ID.to_string()]
            } else {
                ids
            }
        }
        "checkbox" => row_property_checked(row, property)
            .map(|checked| checked.to_string())
            .into_iter()
            .collect(),
        "date" => row_property_payload(row, property)
            .and_then(|payload| {
                payload
                    .get("start")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .filter(|date| !date.trim().is_empty())
            .map(|date| vec![date])
            .unwrap_or_else(|| vec![BOARD_EMPTY_GROUP_ID.to_string()]),
        "people" | "relation" => {
            let ids: Vec<String> = row_property_payload(row, property)
                .and_then(|payload| payload.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                .collect();
            if ids.is_empty() {
                vec![BOARD_EMPTY_GROUP_ID.to_string()]
            } else {
                ids
            }
        }
        _ => vec![BOARD_UNGROUPED_ID.to_string()],
    }
}

fn dynamic_group_label(group_id: &str, group_property: Option<&BoardProperty>) -> (String, String) {
    if group_id == BOARD_EMPTY_GROUP_ID {
        return ("No value".to_string(), "default".to_string());
    }
    if let Some(property) = group_property {
        if property.property_type == "date" {
            return (group_id.to_string(), "blue".to_string());
        }
        if property.property_type == "people" {
            return (format!("Person {group_id}"), "purple".to_string());
        }
        if property.property_type == "relation" {
            return (format!("Related {group_id}"), "default".to_string());
        }
    }
    (group_id.to_string(), "default".to_string())
}

fn board_move_value(property: &BoardProperty, group_id: &str) -> Result<Value, String> {
    match property.property_type.as_str() {
        "select" | "status" => {
            if group_id == BOARD_EMPTY_GROUP_ID {
                Ok(Value::Null)
            } else {
                option_name_by_id(property, group_id).map(Value::String)
            }
        }
        "multi_select" => {
            if group_id == BOARD_EMPTY_GROUP_ID {
                Ok(Value::String(String::new()))
            } else {
                option_name_by_id(property, group_id).map(Value::String)
            }
        }
        "checkbox" => match group_id {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("checkbox board group must be checked or unchecked".to_string()),
        },
        "date" => {
            if group_id == BOARD_EMPTY_GROUP_ID {
                Ok(Value::Null)
            } else {
                Ok(Value::String(group_id.to_string()))
            }
        }
        "people" => {
            if group_id == BOARD_EMPTY_GROUP_ID {
                Ok(Value::Array(Vec::new()))
            } else {
                Err("people board groups can only move cards to no value".to_string())
            }
        }
        "relation" => Err("relation board moves require normalized relation links".to_string()),
        _ => Err("board group property type is not movable".to_string()),
    }
}

fn option_name_by_id(property: &BoardProperty, group_id: &str) -> Result<String, String> {
    property_options(property)
        .into_iter()
        .find(|option| option.id == group_id || option.name.eq_ignore_ascii_case(group_id))
        .map(|option| option.name)
        .ok_or_else(|| "board target group option was not found".to_string())
}

fn row_property_value(row: &NotePageRow, property: &BoardProperty) -> Option<Value> {
    let properties = parse_json(&row.properties, "row page properties").ok()?;
    properties.get(&property.key).cloned()
}

fn row_property_payload(row: &NotePageRow, property: &BoardProperty) -> Option<Value> {
    row_property_value(row, property)?
        .get(&property.property_type)
        .cloned()
}

fn row_property_checked(row: &NotePageRow, property: &BoardProperty) -> Option<bool> {
    let payload = row_property_payload(row, property)?;
    if property.property_type == "formula" {
        return data_sources::formulas::formula_checked(&payload);
    }
    payload.as_bool()
}

fn unique_strings(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.to_string()) {
            result.push(value.to_string());
        }
    }
    result
}

fn string_array(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| "board configuration list must be an array".to_string())?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| "board configuration list item must be text".to_string())
        })
        .collect()
}
