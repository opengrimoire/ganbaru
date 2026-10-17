use crate::data_sources;
use crate::models::NotePageRow;
use serde_json::{Map, Number, Value, json};
use sqlx::{Sqlite, Transaction};
use std::cmp::Ordering;
use std::collections::HashSet;

const MAX_PROPERTY_NAME_CHARS: usize = 120;
const MAX_PROPERTY_ID_CHARS: usize = 80;
const MAX_ROLLUP_VALUE_BYTES: usize = 50 * 1024;
const ROLLUP_FUNCTIONS: &[&str] = &[
    "average",
    "checked",
    "count",
    "count_values",
    "date_range",
    "earliest_date",
    "empty",
    "latest_date",
    "max",
    "median",
    "min",
    "not_empty",
    "percent_checked",
    "percent_empty",
    "percent_not_empty",
    "percent_unchecked",
    "range",
    "show_original",
    "show_unique",
    "sum",
    "unchecked",
    "unique",
];

#[derive(Clone)]
struct RollupConfig {
    relation_property_id: String,
    relation_property_name: String,
    rollup_property_id: String,
    rollup_property_name: String,
    function: String,
}

#[derive(Clone)]
struct SchemaProperty {
    key: String,
    id: String,
    name: String,
    property_type: String,
    schema: Value,
}

#[derive(Clone)]
struct RollupProperty {
    key: String,
    id: String,
    name: String,
    config: RollupConfig,
    relation_property: SchemaProperty,
    target_data_source_id: String,
    target_property: SchemaProperty,
}

pub fn canonical_rollup_config(value: Option<&Value>) -> Result<Value, String> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| "rollup config must be an object".to_string())?;
    let relation_property_id = validate_non_empty_text(
        read_string_field(
            object,
            "relation_property_id",
            "rollup.relation_property_id",
        )?,
        "rollup.relation_property_id",
        MAX_PROPERTY_ID_CHARS,
    )?;
    let relation_property_name = validate_non_empty_text(
        read_string_field(
            object,
            "relation_property_name",
            "rollup.relation_property_name",
        )?,
        "rollup.relation_property_name",
        MAX_PROPERTY_NAME_CHARS,
    )?;
    let rollup_property_id = validate_non_empty_text(
        read_string_field(object, "rollup_property_id", "rollup.rollup_property_id")?,
        "rollup.rollup_property_id",
        MAX_PROPERTY_ID_CHARS,
    )?;
    let rollup_property_name = validate_non_empty_text(
        read_string_field(
            object,
            "rollup_property_name",
            "rollup.rollup_property_name",
        )?,
        "rollup.rollup_property_name",
        MAX_PROPERTY_NAME_CHARS,
    )?;
    let function = read_string_field(object, "function", "rollup.function")?.trim();
    if !ROLLUP_FUNCTIONS.contains(&function) {
        return Err("rollup.function is not supported".to_string());
    }
    Ok(json!({
        "relation_property_id": relation_property_id,
        "relation_property_name": relation_property_name,
        "rollup_property_id": rollup_property_id,
        "rollup_property_name": rollup_property_name,
        "function": function
    }))
}

pub async fn ensure_rollup_schema_targets_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    properties: &Value,
) -> Result<(), String> {
    rollup_properties_tx(tx, source_data_source_id, properties)
        .await
        .map(|_| ())
}

pub async fn invalidate_rollup_cache_for_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM notes_data_source_rollup_cache
         WHERE source_data_source_id = ? OR target_data_source_id = ?",
    )
    .bind(data_source_id)
    .bind(data_source_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("invalidate notes rollup cache: {e}"))?;
    Ok(())
}

pub async fn invalidate_rollup_cache_for_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    let data_source_id: Option<String> = sqlx::query_scalar(
        "SELECT parent_data_source_id
         FROM notes_pages
         WHERE id = ? AND parent_type = 'data_source_id'",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes rollup cache page parent: {e}"))?;
    if let Some(data_source_id) = data_source_id {
        invalidate_rollup_cache_for_data_source_tx(tx, &data_source_id).await?;
    }
    Ok(())
}

pub async fn hydrate_rollups_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    schema_properties: &Value,
    rows: &mut [NotePageRow],
) -> Result<(), String> {
    let rollups = rollup_properties_tx(tx, source_data_source_id, schema_properties).await?;
    if rollups.is_empty() {
        return Ok(());
    }
    for row in rows {
        let mut properties = parse_json(&row.properties, "row page properties")?;
        let properties_object = properties
            .as_object_mut()
            .ok_or_else(|| "row page properties must be an object".to_string())?;
        for rollup in &rollups {
            let value = match cached_rollup_value_tx(tx, &row.id, &rollup.id).await? {
                Some(value) => value,
                None => {
                    let computed = compute_rollup_value_tx(tx, rollup, properties_object).await?;
                    store_rollup_cache_tx(tx, row, rollup, &computed).await?;
                    computed
                }
            };
            properties_object.insert(rollup.key.clone(), value);
        }
        row.properties = properties.to_string();
    }
    Ok(())
}

pub fn rollup_property_value(property_id: &str, payload: Value) -> Value {
    json!({
        "id": property_id,
        "type": "rollup",
        "rollup": payload
    })
}

pub fn rollup_plain_text(payload: &Value) -> String {
    let Some(object) = payload.as_object() else {
        return String::new();
    };
    match object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "number" => object
            .get("number")
            .and_then(Value::as_f64)
            .map(number_to_text)
            .unwrap_or_default(),
        "date" => object
            .get("date")
            .map(date_payload_text)
            .unwrap_or_default(),
        "array" => object
            .get("array")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(property_value_plain_text)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn rollup_number(payload: &Value) -> Option<f64> {
    let object = payload.as_object()?;
    if object.get("type").and_then(Value::as_str) != Some("number") {
        return None;
    }
    object.get("number").and_then(Value::as_f64)
}

fn rollup_properties_from_schema(properties: &Value) -> Result<Vec<SchemaProperty>, String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut result = Vec::with_capacity(object.len());
    for (key, value) in object {
        let property = value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        result.push(SchemaProperty {
            key: key.clone(),
            id: read_string_field(property, "id", "property.id")?.to_string(),
            name: read_string_field(property, "name", "property.name")?.to_string(),
            property_type: read_string_field(property, "type", "property.type")?.to_string(),
            schema: value.clone(),
        });
    }
    Ok(result)
}

async fn rollup_properties_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    properties: &Value,
) -> Result<Vec<RollupProperty>, String> {
    let source_properties = rollup_properties_from_schema(properties)?;
    let mut rollups = Vec::new();
    for property in &source_properties {
        if property.property_type != "rollup" {
            continue;
        }
        let config = rollup_config_from_schema(&property.schema)?;
        let relation_property = source_properties
            .iter()
            .find(|candidate| candidate.id == config.relation_property_id)
            .cloned()
            .ok_or_else(|| "rollup relation property was not found".to_string())?;
        if relation_property.property_type != "relation" {
            return Err("rollup relation property must be a relation".to_string());
        }
        if relation_property.name != config.relation_property_name {
            return Err("rollup relation property name must match".to_string());
        }
        let relation_config =
            data_sources::relations::relation_config_from_schema(&relation_property.schema)?
                .ok_or_else(|| "rollup relation property is invalid".to_string())?;
        let target_schema = if relation_config.target_data_source_id == source_data_source_id {
            properties.clone()
        } else {
            load_data_source_properties_tx(tx, &relation_config.target_data_source_id).await?
        };
        let target_property = rollup_properties_from_schema(&target_schema)?
            .into_iter()
            .find(|candidate| candidate.id == config.rollup_property_id)
            .ok_or_else(|| "rollup target property was not found".to_string())?;
        if target_property.name != config.rollup_property_name {
            return Err("rollup target property name must match".to_string());
        }
        if matches!(target_property.property_type.as_str(), "rollup" | "formula") {
            return Err("rollups cannot target computed properties".to_string());
        }
        validate_rollup_function(&config.function, &target_property.property_type)?;
        rollups.push(RollupProperty {
            key: property.key.clone(),
            id: property.id.clone(),
            name: property.name.clone(),
            config,
            relation_property,
            target_data_source_id: relation_config.target_data_source_id,
            target_property,
        });
    }
    Ok(rollups)
}

fn rollup_config_from_schema(schema: &Value) -> Result<RollupConfig, String> {
    if schema.get("type").and_then(Value::as_str) != Some("rollup") {
        return Err("property is not a rollup".to_string());
    }
    let config = schema
        .get("rollup")
        .and_then(Value::as_object)
        .ok_or_else(|| "rollup config must be an object".to_string())?;
    Ok(RollupConfig {
        relation_property_id: read_string_field(
            config,
            "relation_property_id",
            "rollup.relation_property_id",
        )?
        .to_string(),
        relation_property_name: read_string_field(
            config,
            "relation_property_name",
            "rollup.relation_property_name",
        )?
        .to_string(),
        rollup_property_id: read_string_field(
            config,
            "rollup_property_id",
            "rollup.rollup_property_id",
        )?
        .to_string(),
        rollup_property_name: read_string_field(
            config,
            "rollup_property_name",
            "rollup.rollup_property_name",
        )?
        .to_string(),
        function: read_string_field(config, "function", "rollup.function")?.to_string(),
    })
}

fn validate_rollup_function(function: &str, target_type: &str) -> Result<(), String> {
    match function {
        "count" | "count_values" | "empty" | "not_empty" | "percent_empty"
        | "percent_not_empty" | "show_original" | "show_unique" | "unique" => Ok(()),
        "sum" | "average" | "median" | "min" | "max" | "range" if target_type == "number" => Ok(()),
        "checked" | "unchecked" | "percent_checked" | "percent_unchecked"
            if target_type == "checkbox" =>
        {
            Ok(())
        }
        "earliest_date" | "latest_date" | "date_range" if target_type == "date" => Ok(()),
        _ => Err("rollup.function is not compatible with the target property".to_string()),
    }
}

async fn cached_rollup_value_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_page_id: &str,
    source_property_id: &str,
) -> Result<Option<Value>, String> {
    let cached: Option<String> = sqlx::query_scalar(
        "SELECT value
         FROM notes_data_source_rollup_cache
         WHERE source_page_id = ? AND source_property_id = ?",
    )
    .bind(source_page_id)
    .bind(source_property_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes rollup cache: {e}"))?;
    cached
        .map(|value| parse_json(&value, "cached rollup value"))
        .transpose()
}

async fn store_rollup_cache_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: &NotePageRow,
    rollup: &RollupProperty,
    value: &Value,
) -> Result<(), String> {
    let value_text = value.to_string();
    if value_text.len() > MAX_ROLLUP_VALUE_BYTES {
        return Err("rollup value is too large".to_string());
    }
    sqlx::query(
        "INSERT INTO notes_data_source_rollup_cache (
            source_page_id,
            source_data_source_id,
            source_property_id,
            source_property_name,
            relation_property_id,
            rollup_property_id,
            target_data_source_id,
            value,
            computed_time
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(source_page_id, source_property_id) DO UPDATE SET
            source_property_name = excluded.source_property_name,
            relation_property_id = excluded.relation_property_id,
            rollup_property_id = excluded.rollup_property_id,
            target_data_source_id = excluded.target_data_source_id,
            value = excluded.value,
            computed_time = excluded.computed_time",
    )
    .bind(&row.id)
    .bind(row.parent_data_source_id.as_deref().unwrap_or_default())
    .bind(&rollup.id)
    .bind(&rollup.name)
    .bind(&rollup.config.relation_property_id)
    .bind(&rollup.config.rollup_property_id)
    .bind(&rollup.target_data_source_id)
    .bind(value_text)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("store notes rollup cache: {e}"))?;
    Ok(())
}

async fn compute_rollup_value_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rollup: &RollupProperty,
    source_properties: &Map<String, Value>,
) -> Result<Value, String> {
    let target_ids = relation_target_ids(source_properties, &rollup.relation_property);
    let mut values = Vec::new();
    for target_id in target_ids {
        if let Some(target_row) =
            load_active_target_row_tx(tx, &rollup.target_data_source_id, &target_id).await?
        {
            values.push(target_property_value(&target_row, &rollup.target_property));
        }
    }
    let payload = aggregate_rollup(&rollup.config.function, &values);
    Ok(rollup_property_value(&rollup.id, payload))
}

async fn load_active_target_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    target_data_source_id: &str,
    target_page_id: &str,
) -> Result<Option<NotePageRow>, String> {
    sqlx::query_as::<_, NotePageRow>(
        "SELECT page.*
         FROM notes_pages AS page
         JOIN notes_data_sources AS data_source ON data_source.id = page.parent_data_source_id
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE page.id = ?
           AND page.parent_type = 'data_source_id'
           AND page.parent_data_source_id = ?
           AND page.in_trash = 0
           AND page.archived = 0
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(target_page_id)
    .bind(target_data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes rollup target row: {e}"))
}

fn relation_target_ids(
    source_properties: &Map<String, Value>,
    relation_property: &SchemaProperty,
) -> Vec<String> {
    let value = source_properties.get(&relation_property.key).or_else(|| {
        source_properties.values().find(|value| {
            value.get("id").and_then(Value::as_str) == Some(relation_property.id.as_str())
                && value.get("type").and_then(Value::as_str) == Some("relation")
        })
    });
    value
        .and_then(|value| value.get("relation"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn target_property_value(row: &NotePageRow, property: &SchemaProperty) -> Option<Value> {
    let properties = parse_json(&row.properties, "rollup target row properties").ok()?;
    let object = properties.as_object()?;
    object
        .get(&property.key)
        .filter(|value| property_value_matches(value, property))
        .cloned()
        .or_else(|| {
            object
                .values()
                .find(|value| property_value_matches(value, property))
                .cloned()
        })
}

fn property_value_matches(value: &Value, property: &SchemaProperty) -> bool {
    value.get("id").and_then(Value::as_str) == Some(property.id.as_str())
        && value.get("type").and_then(Value::as_str) == Some(property.property_type.as_str())
}

fn aggregate_rollup(function: &str, values: &[Option<Value>]) -> Value {
    let total = values.len();
    let non_empty = values
        .iter()
        .filter(|value| !is_empty_property_value(value))
        .count();
    match function {
        "count" => number_payload(function, Some(total as f64)),
        "count_values" => number_payload(function, Some(non_empty as f64)),
        "empty" => number_payload(function, Some((total - non_empty) as f64)),
        "not_empty" => number_payload(function, Some(non_empty as f64)),
        "percent_empty" => percent_payload(function, total, total - non_empty),
        "percent_not_empty" => percent_payload(function, total, non_empty),
        "unique" => number_payload(function, Some(unique_values(values).len() as f64)),
        "show_original" => array_payload(function, original_values(values)),
        "show_unique" => array_payload(function, unique_values(values)),
        "checked" => number_payload(function, Some(checked_count(values, true) as f64)),
        "unchecked" => number_payload(function, Some(checked_count(values, false) as f64)),
        "percent_checked" => percent_payload(function, total, checked_count(values, true)),
        "percent_unchecked" => percent_payload(function, total, checked_count(values, false)),
        "sum" => number_payload(
            function,
            numeric_values(values).map(|items| items.into_iter().sum()),
        ),
        "average" => number_payload(function, average(numeric_values(values))),
        "median" => number_payload(function, median(numeric_values(values))),
        "min" => number_payload(function, numeric_values(values).and_then(min_number)),
        "max" => number_payload(function, numeric_values(values).and_then(max_number)),
        "range" => number_payload(function, numeric_values(values).and_then(number_range)),
        "earliest_date" => date_payload(function, date_values(values).and_then(earliest_date)),
        "latest_date" => date_payload(function, date_values(values).and_then(latest_date)),
        "date_range" => date_payload(function, date_values(values).and_then(date_range)),
        _ => json!({
            "type": "unsupported",
            "unsupported": null,
            "function": function
        }),
    }
}

fn number_payload(function: &str, value: Option<f64>) -> Value {
    json!({
        "type": "number",
        "number": value.and_then(finite_json_number).map(Value::Number).unwrap_or(Value::Null),
        "function": function
    })
}

fn percent_payload(function: &str, total: usize, count: usize) -> Value {
    let value = if total == 0 {
        Some(0.0)
    } else {
        Some(count as f64 / total as f64)
    };
    number_payload(function, value)
}

fn array_payload(function: &str, values: Vec<Value>) -> Value {
    json!({
        "type": "array",
        "array": values,
        "function": function
    })
}

fn date_payload(function: &str, value: Option<Value>) -> Value {
    json!({
        "type": "date",
        "date": value.unwrap_or(Value::Null),
        "function": function
    })
}

fn finite_json_number(value: f64) -> Option<Number> {
    if value.is_finite() {
        Number::from_f64(value)
    } else {
        None
    }
}

fn original_values(values: &[Option<Value>]) -> Vec<Value> {
    values
        .iter()
        .filter_map(|value| value.as_ref())
        .filter(|value| !is_empty_json_value(value))
        .cloned()
        .collect()
}

fn unique_values(values: &[Option<Value>]) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for value in original_values(values) {
        let key = property_value_plain_text(&value).to_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        unique.push(value);
    }
    unique
}

fn numeric_values(values: &[Option<Value>]) -> Option<Vec<f64>> {
    let numbers = values
        .iter()
        .filter_map(|value| value.as_ref()?.get("number")?.as_f64())
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if numbers.is_empty() {
        None
    } else {
        Some(numbers)
    }
}

fn checked_count(values: &[Option<Value>], expected: bool) -> usize {
    values
        .iter()
        .filter(|value| {
            value
                .as_ref()
                .and_then(|value| value.get("checkbox"))
                .and_then(Value::as_bool)
                == Some(expected)
        })
        .count()
}

fn average(values: Option<Vec<f64>>) -> Option<f64> {
    let values = values?;
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn median(values: Option<Vec<f64>>) -> Option<f64> {
    let mut values = values?;
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[mid - 1] + values[mid]) / 2.0)
    } else {
        values.get(mid).copied()
    }
}

fn min_number(values: Vec<f64>) -> Option<f64> {
    values.into_iter().reduce(f64::min)
}

fn max_number(values: Vec<f64>) -> Option<f64> {
    values.into_iter().reduce(f64::max)
}

fn number_range(values: Vec<f64>) -> Option<f64> {
    Some(max_number(values.clone())? - min_number(values)?)
}

fn date_values(values: &[Option<Value>]) -> Option<Vec<(String, String)>> {
    let dates = values
        .iter()
        .filter_map(|value| {
            let date = value.as_ref()?.get("date")?.as_object()?;
            let start = date.get("start")?.as_str()?.to_string();
            let end = date
                .get("end")
                .and_then(Value::as_str)
                .unwrap_or(&start)
                .to_string();
            Some((start, end))
        })
        .collect::<Vec<_>>();
    if dates.is_empty() { None } else { Some(dates) }
}

fn earliest_date(values: Vec<(String, String)>) -> Option<Value> {
    values
        .into_iter()
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(start, _)| json!({ "start": start, "end": null, "time_zone": null }))
}

fn latest_date(values: Vec<(String, String)>) -> Option<Value> {
    values
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1))
        .map(|(_, end)| json!({ "start": end, "end": null, "time_zone": null }))
}

fn date_range(values: Vec<(String, String)>) -> Option<Value> {
    let earliest = values.iter().map(|(start, _)| start).min()?.clone();
    let latest = values.iter().map(|(_, end)| end).max()?.clone();
    Some(json!({ "start": earliest, "end": latest, "time_zone": null }))
}

fn is_empty_property_value(value: &Option<Value>) -> bool {
    let Some(value) = value else {
        return true;
    };
    is_empty_json_value(value)
}

fn is_empty_json_value(value: &Value) -> bool {
    let property_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = value.get(property_type).unwrap_or(&Value::Null);
    match payload {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(_) => false,
        Value::Bool(_) | Value::Number(_) => false,
    }
}

fn property_value_plain_text(value: &Value) -> String {
    let property_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = value.get(property_type).unwrap_or(&Value::Null);
    match property_type {
        "title" | "rich_text" => payload
            .as_array()
            .map(|items| rich_text_plain_text(items))
            .unwrap_or_default(),
        "number" => payload.as_f64().map(number_to_text).unwrap_or_default(),
        "checkbox" => payload
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_default(),
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
                    .filter_map(|item| {
                        item.get("title")
                            .or_else(|| item.get("name"))
                            .or_else(|| item.get("id"))
                            .and_then(Value::as_str)
                    })
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
        _ => String::new(),
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

async fn load_data_source_properties_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<Value, String> {
    let properties: String = sqlx::query_scalar(
        "SELECT data_source.properties
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes rollup target data source: {e}"))?
    .ok_or_else(|| "rollup target data source not found".to_string())?;
    parse_json(&properties, "rollup target data source properties")
}

fn validate_non_empty_text(value: &str, label: &str, max_chars: usize) -> Result<String, String> {
    let text = validate_text(value, label, max_chars)?;
    if text.is_empty() {
        return Err(format!("{label} is required"));
    }
    Ok(text)
}

fn validate_text(value: &str, label: &str, max_chars: usize) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label} is too long"));
    }
    Ok(value.trim().to_string())
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
