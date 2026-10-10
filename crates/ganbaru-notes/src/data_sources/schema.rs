use crate::models::{
    NoteDataSourceDto, NoteDataSourceRow, NoteDataSourceSchemaDto, NoteDataSourceSchemaUpdate,
    NoteDatabaseRow, NoteDatabaseViewRow, block_parent_from_database_row,
};
use crate::{assets, data_sources};
use serde_json::{Map, Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const MAX_SCHEMA_BYTES: usize = 50 * 1024;
const MAX_PROPERTIES: usize = 100;
const MAX_PROPERTY_NAME_CHARS: usize = 120;
const MAX_PROPERTY_DESCRIPTION_CHARS: usize = 2000;
const MAX_PROPERTY_ICON_NAME_CHARS: usize = 80;
const MAX_OPTIONS: usize = 100;
const MAX_OPTION_NAME_CHARS: usize = 120;
const MAX_UNIQUE_ID_PREFIX_CHARS: usize = 32;

const SUPPORTED_PROPERTY_TYPES: &[&str] = &[
    "title",
    "rich_text",
    "number",
    "select",
    "multi_select",
    "status",
    "date",
    "checkbox",
    "url",
    "email",
    "phone_number",
    "files",
    "people",
    "created_time",
    "created_by",
    "last_edited_time",
    "last_edited_by",
    "unique_id",
    "place",
    "relation",
    "rollup",
    "formula",
    "button",
];

const EMPTY_CONFIG_TYPES: &[&str] = &[
    "title",
    "rich_text",
    "date",
    "checkbox",
    "url",
    "email",
    "phone_number",
    "files",
    "people",
    "created_time",
    "created_by",
    "last_edited_time",
    "last_edited_by",
    "place",
];

const SELECT_COLORS: &[&str] = &[
    "default", "gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red",
];

const NUMBER_FORMATS: &[&str] = &[
    "number",
    "number_with_commas",
    "percent",
    "dollar",
    "euro",
    "pound",
    "yen",
    "yuan",
    "won",
    "ruble",
    "rupee",
    "franc",
    "real",
    "lira",
    "krona",
    "ringgit",
];

const STATUS_GROUPS: &[&str] = &["To-do", "In progress", "Complete"];

pub async fn list_data_sources(pool: &SqlitePool) -> Result<Vec<NoteDataSourceDto>, String> {
    let rows = sqlx::query_as::<_, NoteDataSourceRow>(
        "SELECT data_source.*
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.in_trash = 0
           AND database.in_trash = 0
         ORDER BY data_source.title COLLATE NOCASE ASC, data_source.id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes data sources: {e}"))?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let database = sqlx::query_as::<_, NoteDatabaseRow>(
            "SELECT * FROM notes_databases WHERE id = ? AND in_trash = 0",
        )
        .bind(&row.database_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("load notes data source database: {e}"))?;
        let database_parent = block_parent_from_database_row(&database)?;
        result.push(NoteDataSourceDto::new(row, database_parent)?);
    }
    Ok(result)
}

pub async fn data_source_schema(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceSchemaDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source schema read: {e}"))?;
    let dto = load_data_source_schema_tx(&mut tx, data_source_id, database_id, view_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source schema read: {e}"))?;
    Ok(dto)
}

pub async fn update_data_source_schema(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    update: NoteDataSourceSchemaUpdate,
) -> Result<NoteDataSourceSchemaDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    let prepared = prepare_schema_update(&update)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source schema update: {e}"))?;
    let target_view =
        update_data_source_schema_tx(&mut tx, data_source_id, database_id, view_id, prepared)
            .await?;
    let dto =
        load_data_source_schema_tx(&mut tx, data_source_id, database_id, Some(&target_view.id))
            .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source schema update: {e}"))?;
    Ok(dto)
}

/// Apply schema invariants and view reconciliation in a caller's transaction.
pub(crate) async fn update_data_source_schema_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    prepared: Value,
) -> Result<NoteDatabaseViewRow, String> {
    let target_view =
        load_table_view_for_schema_tx(tx, data_source_id, database_id, view_id).await?;
    crate::databases::editing_lock::ensure_unlocked_tx(tx, &target_view.database_id).await?;
    crate::project_history::mark_data_source_dirty_tx(tx, data_source_id, "Database schema", false)
        .await?;
    let current = load_data_source_row_tx(tx, data_source_id).await?;
    let current_properties = parse_json(&current.properties, "data source properties")?;
    ensure_title_property_preserved(&current_properties, &prepared)?;
    data_sources::relations::ensure_relation_schema_targets_tx(
        tx,
        data_source_id.trim(),
        &prepared,
    )
    .await?;
    data_sources::rollups::ensure_rollup_schema_targets_tx(tx, data_source_id.trim(), &prepared)
        .await?;
    data_sources::formulas::ensure_formula_schema(&prepared)?;
    data_sources::buttons::ensure_button_schema(&prepared)?;
    sqlx::query(
        "UPDATE notes_data_sources
         SET properties = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0",
    )
    .bind(prepared.to_string())
    .bind(data_source_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("update notes data source properties: {e}"))?;
    reconcile_saved_views_tx(tx, data_source_id, &prepared).await?;
    data_sources::relations::rebuild_data_source_relation_links_tx(
        tx,
        data_source_id.trim(),
        &prepared,
    )
    .await?;
    assets::sync_data_source_property_asset_references_tx(tx, data_source_id.trim(), &prepared)
        .await?;
    data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(tx, data_source_id.trim())
        .await?;
    Ok(target_view)
}

pub(crate) fn prepare_schema_update(update: &NoteDataSourceSchemaUpdate) -> Result<Value, String> {
    let properties = canonical_properties(&update.properties)?;
    let schema_bytes = properties.to_string().len();
    if schema_bytes > MAX_SCHEMA_BYTES {
        return Err("data source schema must not exceed 50KB".to_string());
    }
    Ok(properties)
}

fn canonical_properties(value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "properties must be an object".to_string())?;
    if object.len() > MAX_PROPERTIES {
        return Err("data source schema has too many properties".to_string());
    }
    let mut canonical = Map::new();
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut title_count = 0;
    for (key, property_value) in object {
        let property = canonical_property(key, property_value)?;
        let property_object = property
            .as_object()
            .ok_or_else(|| "property must be an object".to_string())?;
        let id = read_string_field(property_object, "id", "property.id")?.to_string();
        let name = read_string_field(property_object, "name", "property.name")?.to_string();
        let property_type =
            read_string_field(property_object, "type", "property.type")?.to_string();
        let id_key = id.to_lowercase();
        if !ids.insert(id_key) {
            return Err("property ids must be unique".to_string());
        }
        let name_key = name.to_lowercase();
        if !names.insert(name_key) {
            return Err("property names must be unique".to_string());
        }
        if property_type == "title" {
            title_count += 1;
            if id != "title" {
                return Err("title property id must be title".to_string());
            }
        }
        canonical.insert(name, property);
    }
    if title_count != 1 {
        return Err("data source schema must contain exactly one title property".to_string());
    }
    Ok(Value::Object(canonical))
}

fn canonical_property(key: &str, value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "property must be an object".to_string())?;
    let property_type = read_string_field(object, "type", "property.type")?;
    if !SUPPORTED_PROPERTY_TYPES.contains(&property_type) {
        return Err(format!(
            "unsupported data source property type: {property_type}"
        ));
    }
    let id = if property_type == "title" {
        "title".to_string()
    } else {
        validate_non_empty_text(
            read_string_field(object, "id", "property.id")?,
            "property.id",
            80,
            false,
        )?
    };
    let fallback_name = if key.trim().is_empty() {
        "Property"
    } else {
        key
    };
    let raw_name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(fallback_name);
    let name = validate_non_empty_text(raw_name, "property.name", MAX_PROPERTY_NAME_CHARS, false)?;
    let description = validate_text(
        object
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "property.description",
        MAX_PROPERTY_DESCRIPTION_CHARS,
    )?;
    let mut canonical = Map::new();
    canonical.insert("id".to_string(), Value::String(id));
    canonical.insert("name".to_string(), Value::String(name));
    canonical.insert("description".to_string(), Value::String(description));
    canonical.insert("type".to_string(), Value::String(property_type.to_string()));
    if let Some(icon) = canonical_property_icon(object.get("icon"))? {
        canonical.insert("icon".to_string(), icon);
    }
    canonical.insert(
        property_type.to_string(),
        canonical_type_config(property_type, object.get(property_type))?,
    );
    Ok(Value::Object(canonical))
}

/// Keep a property's chosen native icon; an absent or null icon means the property shows its type icon.
fn canonical_property_icon(value: Option<&Value>) -> Result<Option<Value>, String> {
    let icon = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(icon)) => icon,
        Some(_) => return Err("property.icon must be an icon object".to_string()),
    };
    if icon.get("type").and_then(Value::as_str) != Some("icon") {
        return Err("property.icon must be a native icon".to_string());
    }
    crate::validation::validate_native_icon(icon, "property.icon")?;
    let native = icon
        .get("icon")
        .and_then(Value::as_object)
        .ok_or_else(|| "property.icon.icon must be an object".to_string())?;
    let name = read_string_field(native, "name", "property.icon.icon.name")?;
    let is_slug_character = |character: char| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    };
    if name.chars().count() > MAX_PROPERTY_ICON_NAME_CHARS
        || name.starts_with('-')
        || !name.chars().all(is_slug_character)
    {
        return Err("property.icon.icon.name must be an icon slug".to_string());
    }
    let mut canonical_native = Map::new();
    canonical_native.insert("name".to_string(), Value::String(name.to_string()));
    if let Some(color) = native.get("color") {
        canonical_native.insert("color".to_string(), color.clone());
    }
    Ok(Some(json!({ "type": "icon", "icon": canonical_native })))
}

fn canonical_type_config(property_type: &str, value: Option<&Value>) -> Result<Value, String> {
    if EMPTY_CONFIG_TYPES.contains(&property_type) {
        return Ok(json!({}));
    }
    match property_type {
        "number" => canonical_number_config(value),
        "select" | "multi_select" => canonical_options_config(value),
        "status" => canonical_status_config(value),
        "unique_id" => canonical_unique_id_config(value),
        "relation" => data_sources::relations::canonical_relation_config(value),
        "rollup" => data_sources::rollups::canonical_rollup_config(value),
        "formula" => data_sources::formulas::canonical_formula_config(value),
        "button" => data_sources::buttons::canonical_button_config(value),
        _ => Err(format!(
            "unsupported data source property type: {property_type}"
        )),
    }
}

fn canonical_number_config(value: Option<&Value>) -> Result<Value, String> {
    let format = value
        .and_then(Value::as_object)
        .and_then(|object| object.get("format"))
        .and_then(Value::as_str)
        .unwrap_or("number");
    if !NUMBER_FORMATS.contains(&format) {
        return Err("number.format must be a supported format".to_string());
    }
    Ok(json!({ "format": format }))
}

fn canonical_options_config(value: Option<&Value>) -> Result<Value, String> {
    let options = value
        .and_then(Value::as_object)
        .and_then(|object| object.get("options"))
        .and_then(Value::as_array)
        .map(|items| canonical_options(items, None))
        .transpose()?
        .unwrap_or_default();
    Ok(json!({ "options": options }))
}

fn canonical_status_config(value: Option<&Value>) -> Result<Value, String> {
    let object = value.and_then(Value::as_object);
    let options = object
        .and_then(|config| config.get("options"))
        .and_then(Value::as_array)
        .map(|items| canonical_options(items, Some("status")))
        .transpose()?
        .unwrap_or_else(default_status_options);
    let groups = canonical_status_groups(object.and_then(|config| config.get("groups")), &options)?;
    let options: Vec<Value> = options
        .into_iter()
        .map(|mut option| {
            if let Some(object) = option.as_object_mut() {
                object.remove("group");
            }
            option
        })
        .collect();
    Ok(json!({
        "options": options,
        "groups": groups
    }))
}

fn canonical_unique_id_config(value: Option<&Value>) -> Result<Value, String> {
    let prefix = value
        .and_then(Value::as_object)
        .and_then(|object| object.get("prefix"));
    let prefix = match prefix {
        Some(Value::Null) | None => Value::Null,
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Value::Null
            } else {
                Value::String(validate_text(
                    trimmed,
                    "unique_id.prefix",
                    MAX_UNIQUE_ID_PREFIX_CHARS,
                )?)
            }
        }
        Some(_) => return Err("unique_id.prefix must be a string or null".to_string()),
    };
    Ok(json!({ "prefix": prefix }))
}

fn canonical_options(values: &[Value], status_type: Option<&str>) -> Result<Vec<Value>, String> {
    if values.len() > MAX_OPTIONS {
        return Err("property options are limited to 100".to_string());
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut options = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let object = value
            .as_object()
            .ok_or_else(|| "property option must be an object".to_string())?;
        let id = validate_non_empty_text(
            object.get("id").and_then(Value::as_str).unwrap_or_default(),
            "option.id",
            80,
            false,
        )?;
        if !ids.insert(id.to_lowercase()) {
            return Err("option ids must be unique".to_string());
        }
        let name = validate_non_empty_text(
            object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "option.name",
            MAX_OPTION_NAME_CHARS,
            true,
        )?;
        if !names.insert(name.to_lowercase()) {
            return Err("option names must be unique".to_string());
        }
        let color = object
            .get("color")
            .and_then(Value::as_str)
            .unwrap_or(default_option_color(index));
        if !SELECT_COLORS.contains(&color) {
            return Err("option.color must be a supported color".to_string());
        }
        let mut option = json!({
            "id": id,
            "name": name,
            "color": color
        });
        if status_type.is_some() {
            let group = object
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or("To-do");
            if !STATUS_GROUPS.contains(&group) {
                return Err("status option group must be supported".to_string());
            }
            option["group"] = Value::String(group.to_string());
        }
        options.push(option);
    }
    Ok(options)
}

fn canonical_status_groups(value: Option<&Value>, options: &[Value]) -> Result<Vec<Value>, String> {
    let option_ids: HashSet<String> = options
        .iter()
        .filter_map(|option| option.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    let mut grouped: HashMap<String, Vec<String>> = STATUS_GROUPS
        .iter()
        .map(|group| ((*group).to_string(), Vec::new()))
        .collect();
    if let Some(groups) = value.and_then(Value::as_array) {
        for group in groups {
            let object = group
                .as_object()
                .ok_or_else(|| "status group must be an object".to_string())?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !STATUS_GROUPS.contains(&name) {
                return Err("status group name must be supported".to_string());
            }
            let ids = object
                .get("option_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| "status group option_ids must be an array".to_string())?;
            for id in ids {
                let id = id
                    .as_str()
                    .ok_or_else(|| "status group option id must be a string".to_string())?;
                if !option_ids.contains(id) {
                    return Err("status group option id must reference an option".to_string());
                }
                grouped
                    .entry(name.to_string())
                    .or_default()
                    .push(id.to_string());
            }
        }
    } else {
        for option in options {
            let group = option
                .get("group")
                .and_then(Value::as_str)
                .filter(|group| STATUS_GROUPS.contains(group))
                .unwrap_or("To-do");
            if let Some(id) = option.get("id").and_then(Value::as_str) {
                grouped
                    .entry(group.to_string())
                    .or_default()
                    .push(id.to_string());
            }
        }
    }
    Ok(STATUS_GROUPS
        .iter()
        .map(|group| {
            json!({
                "id": group,
                "name": group,
                "color": status_group_color(group),
                "option_ids": grouped.remove(*group).unwrap_or_default()
            })
        })
        .collect())
}

fn default_status_options() -> Vec<Value> {
    vec![
        json!({
            "id": "not_started",
            "name": "Not started",
            "color": "default",
            "group": "To-do"
        }),
        json!({
            "id": "in_progress",
            "name": "In progress",
            "color": "blue",
            "group": "In progress"
        }),
        json!({
            "id": "done",
            "name": "Done",
            "color": "green",
            "group": "Complete"
        }),
    ]
}

fn status_group_color(group: &str) -> &'static str {
    match group {
        "In progress" => "blue",
        "Complete" => "green",
        _ => "gray",
    }
}

fn default_option_color(index: usize) -> &'static str {
    SELECT_COLORS
        .get(index % SELECT_COLORS.len())
        .copied()
        .unwrap_or("default")
}

fn ensure_title_property_preserved(current: &Value, next: &Value) -> Result<(), String> {
    let current_title = title_property_name(current)?;
    let next_title = title_property_name(next)?
        .ok_or_else(|| "data source schema must contain exactly one title property".to_string())?;
    if next_title.trim().is_empty() {
        return Err("title property name is required".to_string());
    }
    if current_title.is_none() {
        return Ok(());
    }
    Ok(())
}

fn title_property_name(properties: &Value) -> Result<Option<String>, String> {
    for property in properties
        .as_object()
        .ok_or_else(|| "properties must be an object".to_string())?
        .values()
    {
        let object = property
            .as_object()
            .ok_or_else(|| "property must be an object".to_string())?;
        if object.get("type").and_then(Value::as_str) == Some("title") {
            return Ok(object
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string));
        }
    }
    Ok(None)
}

/// Reconcile source references using each view's current persisted presentation.
async fn reconcile_saved_views_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    properties: &Value,
) -> Result<(), String> {
    let schema = data_sources::views::view_schema(properties)?;
    let property_ids: HashSet<String> = schema.iter().map(|property| property.id.clone()).collect();
    let property_types: HashMap<&str, &str> = schema
        .iter()
        .map(|property| (property.id.as_str(), property.property_type.as_str()))
        .collect();
    let views = sqlx::query_as::<_, NoteDatabaseViewRow>(
        "SELECT *, type AS view_type FROM notes_database_views WHERE data_source_id = ?",
    )
    .bind(data_source_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes views for schema reconciliation: {e}"))?;
    for view in views {
        let mut filters = data_sources::views::stored_filters(
            view.filter.as_deref(),
            "saved view filters",
            &view.view_type,
        )?;
        data_sources::views::reconcile_filters(&mut filters, &property_types);
        let filter =
            data_sources::views::canonical_filter(&filters, &property_types, &view.view_type)?;
        let mut sorts =
            data_sources::views::stored_sorts(&view.sorts, "saved view sorts", &view.view_type)?;
        sorts.retain(|sort| property_ids.contains(sort.property_id.as_str()));
        let sorts = data_sources::views::canonical_sorts(&sorts, &property_ids, &view.view_type)?;
        let configuration = reconcile_view_configuration(&view, &property_ids, &property_types)?;
        let filter = filter.map(|value| value.to_string());
        let sorts = sorts.to_string();
        let configuration = configuration.map(|value| value.to_string());
        if filter == view.filter && sorts == view.sorts && configuration == view.configuration {
            continue;
        }
        sqlx::query(
            "UPDATE notes_database_views
             SET filter = ?, sorts = ?, configuration = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(filter)
        .bind(sorts)
        .bind(configuration)
        .bind(&view.id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("reconcile notes saved view {}: {e}", view.id))?;
    }
    Ok(())
}

/// Preserve display choices while removing references to deleted properties.
fn reconcile_view_configuration(
    view: &NoteDatabaseViewRow,
    property_ids: &HashSet<String>,
    property_types: &HashMap<&str, &str>,
) -> Result<Option<Value>, String> {
    let Some(configuration) = view.configuration.as_deref() else {
        return Ok(None);
    };
    let mut configuration = parse_json(configuration, "saved view configuration")?;
    let Some(settings) = configuration
        .get_mut(&view.view_type)
        .and_then(Value::as_object_mut)
    else {
        return Ok(Some(configuration));
    };
    if view.view_type == "table" {
        let mut order = vec![Value::String("title".to_string())];
        let mut seen = HashSet::from(["title".to_string()]);
        if let Some(current) = settings.get("property_order").and_then(Value::as_array) {
            for value in current {
                if let Some(id) = value.as_str()
                    && property_ids.contains(id)
                    && seen.insert(id.to_string())
                {
                    order.push(value.clone());
                }
            }
        }
        let mut added: Vec<&String> = property_ids
            .iter()
            .filter(|id| !seen.contains(*id))
            .collect();
        added.sort();
        order.extend(added.into_iter().map(|id| Value::String(id.clone())));
        settings.insert("property_order".to_string(), Value::Array(order));
    }
    for key in ["hidden_property_ids", "visible_property_ids"] {
        if let Some(ids) = settings.get_mut(key).and_then(Value::as_array_mut) {
            ids.retain(|value| {
                value
                    .as_str()
                    .is_some_and(|id| id != "title" && property_ids.contains(id))
            });
        }
    }
    if let Some(widths) = settings
        .get_mut("column_widths")
        .and_then(Value::as_object_mut)
    {
        widths.retain(|id, _| property_ids.contains(id));
    }
    if view.view_type == "table" {
        if let Some(presentation) = settings
            .get_mut("presentation")
            .and_then(Value::as_object_mut)
        {
            if presentation
                .get("frozen_property_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !property_ids.contains(id))
            {
                presentation.insert("frozen_property_id".to_string(), Value::Null);
            }
            if let Some(columns) = presentation
                .get_mut("columns")
                .and_then(Value::as_object_mut)
            {
                columns.retain(|id, _| property_ids.contains(id));
                for (id, column) in columns {
                    let Some(column) = column.as_object_mut() else {
                        continue;
                    };
                    let calculation = column.get("calculation").and_then(Value::as_str);
                    let property_type = property_types.get(id.as_str()).copied().unwrap_or("");
                    let incompatible = match calculation {
                        Some("sum" | "average" | "min" | "max") => {
                            !matches!(property_type, "number" | "formula" | "rollup")
                        }
                        Some("percent_checked") => property_type != "checkbox",
                        _ => false,
                    };
                    if incompatible {
                        column.insert("calculation".to_string(), Value::Null);
                    }
                }
            }
            if let Some(rules) = presentation
                .get_mut("color_rules")
                .and_then(Value::as_array_mut)
            {
                rules.retain_mut(|rule| {
                    if rule
                        .get("property_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !property_ids.contains(id))
                    {
                        return false;
                    }
                    let Some(filters) = rule.get("filters") else {
                        return false;
                    };
                    let Ok(mut filters) = serde_json::from_value::<
                        Vec<crate::models::NoteDataSourceTableFilter>,
                    >(filters.clone()) else {
                        return false;
                    };
                    data_sources::views::reconcile_filters(&mut filters, property_types);
                    if filters.is_empty() {
                        return false;
                    }
                    if let Some(rule) = rule.as_object_mut() {
                        rule.insert("filters".to_string(), json!(filters));
                    }
                    true
                });
            }
        }
    }
    for key in ["group_property_id", "date_property_id", "cover_property_id"] {
        let invalid = settings.get(key).and_then(Value::as_str).is_some_and(|id| {
            match property_types.get(id) {
                Some(property_type) => match key {
                    "group_property_id" => {
                        !data_sources::views::GROUP_PROPERTY_TYPES.contains(property_type)
                    }
                    "date_property_id" => *property_type != "date",
                    "cover_property_id" => *property_type != "files",
                    _ => false,
                },
                None => true,
            }
        });
        if !invalid {
            continue;
        }
        settings.insert(key.to_string(), Value::Null);
        if key == "group_property_id" {
            settings.insert("group_order".to_string(), json!([]));
            settings.insert("hidden_group_ids".to_string(), json!([]));
            if view.view_type == "table" {
                settings.insert("collapsed_group_ids".to_string(), json!([]));
            }
        } else if key == "cover_property_id" {
            settings.insert("cover_source".to_string(), json!("none"));
        }
    }
    Ok(Some(configuration))
}

pub(crate) async fn load_data_source_schema_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceSchemaDto, String> {
    let data_source = load_data_source_row_tx(tx, data_source_id).await?;
    let database =
        sqlx::query_as::<_, NoteDatabaseRow>("SELECT * FROM notes_databases WHERE id = ?")
            .bind(&data_source.database_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("load notes data source database: {e}"))?;
    let view = load_table_view_for_schema_tx(tx, data_source_id, database_id, view_id).await?;
    NoteDataSourceSchemaDto::new(data_source, database, view)
}

async fn load_table_view_for_schema_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDatabaseViewRow, String> {
    let view = if let Some(view_id) = view_id {
        data_sources::views::load_scoped_view_row_tx(
            tx,
            data_source_id,
            "table",
            database_id,
            Some(view_id),
        )
        .await?
    } else {
        data_sources::views::load_scoped_view_row_tx(tx, data_source_id, "table", database_id, None)
            .await?
    };
    view.ok_or_else(|| "data source table view not found".to_string())
}

async fn load_data_source_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<NoteDataSourceRow, String> {
    sqlx::query_as::<_, NoteDataSourceRow>(
        "SELECT *
         FROM notes_data_sources
         WHERE id = ? AND in_trash = 0",
    )
    .bind(data_source_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source: {e}"))?
    .ok_or_else(|| "data source not found".to_string())
}

fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}

fn read_string_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} must be a string"))
}

fn validate_non_empty_text(
    value: &str,
    label: &str,
    max_chars: usize,
    reject_comma: bool,
) -> Result<String, String> {
    let trimmed = validate_text(value.trim(), label, max_chars)?;
    if trimmed.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if reject_comma && trimmed.contains(',') {
        return Err(format!("{label} must not contain commas"));
    }
    Ok(trimmed)
}

fn validate_text(value: &str, label: &str, max_chars: usize) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label} is too long"));
    }
    Ok(value.to_string())
}
