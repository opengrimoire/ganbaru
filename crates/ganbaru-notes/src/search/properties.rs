use crate::data_sources;
use crate::models::NotePageRow;
use serde_json::{Map, Value};
use sqlx::{FromRow, SqlitePool};
use std::collections::HashMap;

pub struct PropertySearchEntry {
    pub id: String,
    pub page_id: String,
    pub property_id: String,
    pub title: String,
    pub body: String,
    pub metadata: String,
    pub source_last_edited_time: String,
}

pub async fn property_search_entries(
    pool: &SqlitePool,
) -> Result<Vec<PropertySearchEntry>, String> {
    let data_sources = sqlx::query_as::<_, PropertySearchDataSourceRow>(
        "SELECT data_source.id,
            data_source.properties,
            data_source.last_edited_time
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes data sources for property search: {e}"))?;

    let mut entries = Vec::new();
    for data_source in data_sources {
        let schema_properties = parse_json(&data_source.properties, "data source properties")?;
        let schema = search_schema(&schema_properties)?;
        if schema.is_empty() {
            continue;
        }
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| format!("begin notes property search read: {e}"))?;
        let mut rows = load_active_row_pages_tx(&mut tx, &data_source.id).await?;
        data_sources::relations::hydrate_relation_titles_tx(&mut tx, &mut rows).await?;
        let cache_times =
            apply_cached_rollups_tx(&mut tx, &data_source.id, &schema, &mut rows).await?;
        tx.commit()
            .await
            .map_err(|e| format!("commit notes property search read: {e}"))?;
        data_sources::formulas::hydrate_formulas(&schema_properties, &mut rows)?;
        append_row_property_entries(&mut entries, &data_source, &schema, rows, &cache_times)?;
    }
    Ok(entries)
}

async fn load_active_row_pages_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    data_source_id: &str,
) -> Result<Vec<NotePageRow>, String> {
    sqlx::query_as::<_, NotePageRow>(
        "SELECT page.*
         FROM notes_pages AS page
         WHERE page.parent_type = 'data_source_id'
           AND page.parent_data_source_id = ?
           AND page.in_trash = 0
           AND page.archived = 0",
    )
    .bind(data_source_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes row pages for property search: {e}"))
}

async fn apply_cached_rollups_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    data_source_id: &str,
    schema: &[SearchProperty],
    rows: &mut [NotePageRow],
) -> Result<HashMap<(String, String), String>, String> {
    let rollup_properties = schema
        .iter()
        .filter(|property| property.property_type == "rollup")
        .map(|property| (property.id.clone(), property.clone()))
        .collect::<HashMap<_, _>>();
    if rollup_properties.is_empty() || rows.is_empty() {
        return Ok(HashMap::new());
    }

    let row_indexes = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let cache_rows = sqlx::query_as::<_, RollupCacheRow>(
        "SELECT source_page_id,
            source_property_id,
            value,
            computed_time
         FROM notes_data_source_rollup_cache
         WHERE source_data_source_id = ?",
    )
    .bind(data_source_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes rollup cache for property search: {e}"))?;

    let mut cache_times = HashMap::new();
    for cache in cache_rows {
        let Some(row_index) = row_indexes.get(&cache.source_page_id).copied() else {
            continue;
        };
        let Some(property) = rollup_properties.get(&cache.source_property_id) else {
            continue;
        };
        let value = parse_json(&cache.value, "cached rollup value")?;
        if !property_value_matches_schema(&value, property) {
            continue;
        }
        let mut properties = parse_json(&rows[row_index].properties, "row page properties")?;
        let object = properties
            .as_object_mut()
            .ok_or_else(|| "row page properties must be an object".to_string())?;
        object.insert(property.key.clone(), value);
        rows[row_index].properties = properties.to_string();
        cache_times.insert(
            (cache.source_page_id, cache.source_property_id),
            cache.computed_time,
        );
    }
    Ok(cache_times)
}

fn append_row_property_entries(
    entries: &mut Vec<PropertySearchEntry>,
    data_source: &PropertySearchDataSourceRow,
    schema: &[SearchProperty],
    rows: Vec<NotePageRow>,
    cache_times: &HashMap<(String, String), String>,
) -> Result<(), String> {
    for row in rows {
        let properties = parse_json(&row.properties, "row page properties")?;
        let object = properties
            .as_object()
            .ok_or_else(|| "row page properties must be an object".to_string())?;
        for property in schema {
            let Some(value) = property_value(object, property) else {
                continue;
            };
            let body = property_value_plain_text(value, &property.property_type);
            if body.trim().is_empty() {
                continue;
            }
            let cache_time = cache_times.get(&(row.id.clone(), property.id.clone()));
            let source_last_edited_time = cache_time
                .map(|time| latest_time(&row.last_edited_time, time))
                .unwrap_or_else(|| row.last_edited_time.clone());
            entries.push(PropertySearchEntry {
                id: format!("property_value:{}:{}", row.id, property.id),
                page_id: row.id.clone(),
                property_id: property.id.clone(),
                title: property.name.clone(),
                body,
                metadata: format!(
                    "database property {}",
                    property.property_type.replace('_', " ")
                ),
                source_last_edited_time: latest_time(
                    &source_last_edited_time,
                    &data_source.last_edited_time,
                ),
            });
        }
    }
    Ok(())
}

fn search_schema(properties: &Value) -> Result<Vec<SearchProperty>, String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut schema = Vec::with_capacity(object.len());
    for (key, value) in object {
        let property = value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        schema.push(SearchProperty {
            key: key.clone(),
            id: read_string_field(property, "id", "property.id")?.to_string(),
            name: read_string_field(property, "name", "property.name")?.to_string(),
            property_type: read_string_field(property, "type", "property.type")?.to_string(),
        });
    }
    Ok(schema)
}

fn property_value<'a>(
    properties: &'a Map<String, Value>,
    property: &SearchProperty,
) -> Option<&'a Value> {
    properties
        .get(&property.key)
        .filter(|value| property_value_matches_schema(value, property))
        .or_else(|| {
            properties
                .values()
                .find(|value| property_value_matches_schema(value, property))
        })
}

fn property_value_matches_schema(value: &Value, property: &SearchProperty) -> bool {
    value.get("id").and_then(Value::as_str) == Some(property.id.as_str())
        && value.get("type").and_then(Value::as_str) == Some(property.property_type.as_str())
}

fn property_value_plain_text(value: &Value, property_type: &str) -> String {
    let payload = value.get(property_type).unwrap_or(&Value::Null);
    match property_type {
        "title" | "rich_text" => payload
            .as_array()
            .map(|items| rich_text_plain_text(items))
            .unwrap_or_default(),
        "number" => payload.as_f64().map(number_to_text).unwrap_or_default(),
        "checkbox" => checkbox_text(payload.as_bool()),
        "select" | "status" | "place" => payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "multi_select" | "people" | "relation" | "files" => payload
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(property_list_item_text)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        "date" => date_payload_text(payload),
        "url" | "email" | "phone_number" | "created_time" | "last_edited_time" => {
            payload.as_str().unwrap_or_default().to_string()
        }
        "unique_id" => {
            let prefix = payload.get("prefix").and_then(Value::as_str).unwrap_or("");
            let number = payload
                .get("number")
                .and_then(Value::as_i64)
                .map(|value| value.to_string())
                .unwrap_or_default();
            format!("{prefix}{number}")
        }
        "rollup" => data_sources::rollups::rollup_plain_text(payload),
        "formula" => formula_payload_text(payload),
        _ => String::new(),
    }
}

fn formula_payload_text(payload: &Value) -> String {
    if payload.get("ganbaru_error").is_some() {
        return String::new();
    }
    if let Some(checked) = data_sources::formulas::formula_checked(payload) {
        return checkbox_text(Some(checked));
    }
    data_sources::formulas::formula_plain_text(payload)
}

fn property_list_item_text(value: &Value) -> Option<String> {
    value
        .get("title")
        .or_else(|| value.get("name"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn checkbox_text(value: Option<bool>) -> String {
    match value {
        Some(true) => "true checked yes".to_string(),
        Some(false) => "false unchecked no".to_string(),
        None => String::new(),
    }
}

fn rich_text_plain_text(items: &[Value]) -> String {
    let mut text = String::new();
    for item in items {
        if let Some(plain_text) = item.get("plain_text").and_then(Value::as_str) {
            text.push_str(plain_text);
        } else if let Some(content) = item
            .get("text")
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str)
        {
            text.push_str(content);
        }
    }
    text
}

fn date_payload_text(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return String::new();
    };
    let start = object
        .get("start")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let end = object
        .get("end")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if end.is_empty() || end == start {
        start.to_string()
    } else {
        format!("{start} to {end}")
    }
}

fn number_to_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn latest_time(left: &str, right: &str) -> String {
    if right > left {
        right.to_string()
    } else {
        left.to_string()
    }
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

#[derive(Clone)]
struct SearchProperty {
    key: String,
    id: String,
    name: String,
    property_type: String,
}

#[derive(FromRow)]
struct PropertySearchDataSourceRow {
    id: String,
    properties: String,
    last_edited_time: String,
}

#[derive(FromRow)]
struct RollupCacheRow {
    source_page_id: String,
    source_property_id: String,
    value: String,
    computed_time: String,
}
