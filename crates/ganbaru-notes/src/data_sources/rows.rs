use crate::block_storage::{BlockRowInsert, insert_block_row};
use crate::models::{NoteDataSourceRowPageCreate, NoteLoadedPage, NotePageDto, NotePageRow};
use crate::validation::{plain_text_from_payload, require_uuid};
use crate::{assets, data_sources, reads, writes};
use ganbaru_sync_contracts::OrderKey;
use serde_json::{Map, Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};

const MAX_ROW_PROPERTIES_BYTES: usize = 50 * 1024;
const MAX_ROW_TITLE_CHARS: usize = 2000;

pub async fn list_data_source_row_pages(
    pool: &SqlitePool,
    data_source_id: &str,
) -> Result<Vec<NotePageDto>, String> {
    require_uuid(data_source_id, "data_source_id")?;
    let rows = sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {}
         FROM notes_pages AS page
         JOIN notes_data_sources AS data_source ON data_source.id = page.parent_data_source_id
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE page.parent_type = 'data_source_id'
           AND page.parent_data_source_id = ?
           AND page.in_trash = 0
           AND page.archived = 0
           AND data_source.in_trash = 0
           AND database.in_trash = 0
         ORDER BY page.last_edited_time DESC, page.title COLLATE NOCASE ASC, page.id ASC",
        crate::structure::page_row_columns("page")
    )))
    .bind(data_source_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes data source row pages: {e}"))?;
    rows.into_iter().map(NotePageDto::new).collect()
}

pub async fn create_data_source_row_page(
    pool: &SqlitePool,
    data_source_id: &str,
    request: NoteDataSourceRowPageCreate,
) -> Result<NoteLoadedPage, String> {
    let data_source_id = data_source_id.trim();
    require_uuid(data_source_id, "data_source_id")?;
    require_uuid(&request.id, "id")?;
    require_uuid(&request.first_block_id, "first_block_id")?;
    if request.id.trim() == request.first_block_id.trim() {
        return Err("first_block_id must not match id".to_string());
    }
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source row create: {e}"))?;
    create_data_source_row_page_tx(&mut tx, data_source_id, &request).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source row create: {e}"))?;
    reads::load_page(pool, request.id.trim()).await
}

/// Create a canonical row document inside an existing atomic source mutation.
pub(crate) async fn create_data_source_row_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    request: &NoteDataSourceRowPageCreate,
) -> Result<(), String> {
    require_uuid(data_source_id, "data_source_id")?;
    require_uuid(&request.id, "id")?;
    require_uuid(&request.first_block_id, "first_block_id")?;
    if request.id.trim() == request.first_block_id.trim() {
        return Err("first_block_id must not match id".to_string());
    }
    let data_source = load_active_data_source_tx(tx, data_source_id).await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let (title, properties) = row_page_properties(
        &schema_properties,
        &request.title,
        request.properties.as_ref(),
    )?;
    let first_payload = writes::default_text_payload("");
    let first_plain_text = plain_text_from_payload("paragraph", &first_payload);
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_data_source_id,
            title,
            properties
         )
         VALUES (?, 'data_source_id', ?, ?, ?)",
    )
    .bind(request.id.trim())
    .bind(data_source_id)
    .bind(&title)
    .bind(properties.to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create notes data source row page: {e}"))?;
    let first_order_key = OrderKey::first().into_string();
    insert_block_row(
        tx,
        BlockRowInsert {
            id: request.first_block_id.trim(),
            page_id: request.id.trim(),
            parent_type: "page_id",
            parent_page_id: Some(request.id.trim()),
            parent_block_id: None,
            block_type: "paragraph",
            payload: &first_payload,
            plain_text: &first_plain_text,
            order_key: &first_order_key,
            source_provider: None,
            source_object_id: None,
            source_last_edited_time: None,
        },
    )
    .await
    .map_err(|e| format!("create initial notes data source row block: {e}"))?;
    data_sources::relations::replace_row_relation_links_tx(
        tx,
        data_source_id,
        request.id.trim(),
        &schema_properties,
        &properties,
        true,
    )
    .await?;
    assets::sync_data_source_property_asset_references_tx(tx, data_source_id, &schema_properties)
        .await?;
    data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(tx, data_source_id).await?;
    touch_data_source_tx(tx, data_source_id, &data_source.database_id).await?;
    Ok(())
}

struct DataSourceRow {
    database_id: String,
    properties: String,
}

async fn load_active_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<DataSourceRow, String> {
    sqlx::query_as::<_, DataSourceRow>(concat!(
        "SELECT data_source.database_id, ",
        crate::note_data_source_properties_json!("data_source"),
        " AS properties
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0"
    ))
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source for row page: {e}"))?
    .ok_or_else(|| "data source not found".to_string())
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for DataSourceRow {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            database_id: row.try_get("database_id")?,
            properties: row.try_get("properties")?,
        })
    }
}

async fn touch_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: &str,
) -> Result<(), String> {
    crate::project_history::mark_data_source_dirty_tx(tx, data_source_id, "Database row", false)
        .await?;
    sqlx::query(
        "UPDATE notes_data_sources
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(data_source_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes data source after row create: {e}"))?;
    sqlx::query(
        "UPDATE notes_databases
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(database_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes database after row create: {e}"))?;
    Ok(())
}

pub fn row_page_properties(
    schema_properties: &Value,
    title: &str,
    provided: Option<&Value>,
) -> Result<(String, Value), String> {
    let requested_title = normalize_row_title(title)?;
    let schema = schema_properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let provided_properties = provided
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| "row properties must be an object".to_string())
        })
        .transpose()?;
    if let Some(provided_object) = provided_properties {
        for key in provided_object.keys() {
            if !schema.contains_key(key) {
                return Err("row properties contain an unknown property".to_string());
            }
        }
    }

    let mut resolved_title = requested_title.clone();
    let mut has_title_property = false;
    let mut row_properties = Map::new();
    for (name, schema_value) in schema {
        let schema_object = schema_value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        let property_id = read_string_field(schema_object, "id", "property.id")?;
        let property_type = read_string_field(schema_object, "type", "property.type")?;
        if matches!(property_type, "rollup" | "formula" | "button") {
            continue;
        }
        let value = match provided_properties.and_then(|properties| properties.get(name)) {
            Some(property_value) => {
                canonical_row_property_value(property_id, property_type, property_value)?
            }
            None => default_row_property_value(schema_object, &requested_title)?,
        };
        if property_type == "title" {
            has_title_property = true;
            resolved_title = title_from_property_value(&value)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| requested_title.clone());
        }
        row_properties.insert(name.clone(), value);
    }
    if !has_title_property {
        return Err("data source schema must contain a title property".to_string());
    }
    let row_value = Value::Object(row_properties);
    if row_value.to_string().len() > MAX_ROW_PROPERTIES_BYTES {
        return Err("row properties must not exceed 50KB".to_string());
    }
    Ok((normalize_row_title(&resolved_title)?, row_value))
}

fn canonical_row_property_value(
    property_id: &str,
    property_type: &str,
    value: &Value,
) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "row property value must be an object".to_string())?;
    let stored_id = object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(property_id);
    if stored_id != property_id {
        return Err("row property id must match the data source schema".to_string());
    }
    let stored_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(property_type);
    if stored_type != property_type {
        return Err("row property type must match the data source schema".to_string());
    }
    let raw_property_value = object
        .get(property_type)
        .ok_or_else(|| "row property value is missing its typed payload".to_string())?;
    let payload = canonical_property_payload(property_type, raw_property_value)?;
    if property_type == "relation" {
        return Ok(data_sources::relations::relation_property_value(
            property_id,
            payload,
        ));
    }
    Ok(json!({
        "id": property_id,
        "type": property_type,
        property_type: payload
    }))
}

fn default_row_property_value(
    schema_object: &Map<String, Value>,
    title: &str,
) -> Result<Value, String> {
    let property_id = read_string_field(schema_object, "id", "property.id")?;
    let property_type = read_string_field(schema_object, "type", "property.type")?;
    let value = match property_type {
        "title" => Value::Array(vec![writes::rich_text(title)]),
        "rich_text" | "multi_select" | "files" | "people" | "relation" => Value::Array(Vec::new()),
        "status" => default_status_option(schema_object),
        "number" | "select" | "date" | "url" | "email" | "phone_number" | "created_time"
        | "created_by" | "last_edited_time" | "last_edited_by" | "place" => Value::Null,
        "checkbox" => Value::Bool(false),
        "unique_id" => json!({
            "number": null,
            "prefix": schema_object
                .get("unique_id")
                .and_then(|config| config.get("prefix"))
                .cloned()
                .unwrap_or(Value::Null)
        }),
        other => return Err(format!("unsupported row property type: {other}")),
    };
    if property_type == "relation" {
        return Ok(data_sources::relations::relation_property_value(
            property_id,
            value,
        ));
    }
    Ok(json!({
        "id": property_id,
        "type": property_type,
        property_type: value
    }))
}

/// The status option a new row starts with, matching the stored option shape.
fn default_status_option(schema_object: &Map<String, Value>) -> Value {
    let config = schema_object.get("status");
    let Some(default_id) = config
        .and_then(|config| config.get("default_option_id"))
        .and_then(Value::as_str)
    else {
        return Value::Null;
    };
    config
        .and_then(|config| config.get("options"))
        .and_then(Value::as_array)
        .and_then(|options| {
            options
                .iter()
                .find(|option| option.get("id").and_then(Value::as_str) == Some(default_id))
        })
        .map_or(Value::Null, |option| {
            json!({
                "id": default_id,
                "name": option.get("name").cloned().unwrap_or(Value::Null),
                "color": option.get("color").cloned().unwrap_or(json!("default"))
            })
        })
}

fn canonical_property_payload(property_type: &str, value: &Value) -> Result<Value, String> {
    match property_type {
        "title" | "rich_text" => {
            if !value.is_array() {
                return Err("row rich text property values must be arrays".to_string());
            }
            Ok(value.clone())
        }
        "number" => {
            if value.is_null() || value.is_number() {
                Ok(value.clone())
            } else {
                Err("row number property must be a number or null".to_string())
            }
        }
        "select" | "status" | "date" | "created_by" | "last_edited_by" | "unique_id" | "place" => {
            if value.is_null() || value.is_object() {
                Ok(value.clone())
            } else {
                Err(format!(
                    "row {property_type} property must be an object or null"
                ))
            }
        }
        "multi_select" | "files" | "people" => {
            if value.is_array() {
                Ok(value.clone())
            } else {
                Err(format!("row {property_type} property must be an array"))
            }
        }
        "relation" => data_sources::relations::canonical_relation_payload(value),
        "checkbox" => {
            if let Some(checked) = value.as_bool() {
                Ok(Value::Bool(checked))
            } else {
                Err("row checkbox property must be a boolean".to_string())
            }
        }
        "url" | "email" | "phone_number" | "created_time" | "last_edited_time" => {
            if value.is_null() {
                return Ok(Value::Null);
            }
            let text = value
                .as_str()
                .ok_or_else(|| format!("row {property_type} property must be a string or null"))?;
            if text.chars().any(char::is_control) {
                return Err(format!(
                    "row {property_type} property must not contain control characters"
                ));
            }
            Ok(Value::String(text.to_string()))
        }
        other => Err(format!("unsupported row property type: {other}")),
    }
}

fn title_from_property_value(value: &Value) -> Option<String> {
    let title_items = value.get("title")?.as_array()?;
    let mut title = String::new();
    for item in title_items {
        if let Some(plain_text) = item.get("plain_text").and_then(Value::as_str) {
            title.push_str(plain_text);
            continue;
        }
        if let Some(content) = item
            .get("text")
            .and_then(|text| text.get("content"))
            .and_then(Value::as_str)
        {
            title.push_str(content);
        }
    }
    Some(title)
}

fn normalize_row_title(title: &str) -> Result<String, String> {
    let trimmed = title.trim();
    if trimmed.chars().any(char::is_control) {
        return Err("title must not contain control characters".to_string());
    }
    if trimmed.chars().count() > MAX_ROW_TITLE_CHARS {
        return Err("title is too long".to_string());
    }
    Ok(trimmed.to_string())
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

fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}
