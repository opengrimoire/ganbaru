use crate::models::NotePageRow;
use crate::validation::require_uuid;
use crate::{data_sources, page_property_storage};
use serde_json::{Map, Value, json};
use sqlx::{Sqlite, Transaction};
use std::collections::{HashMap, HashSet};

const MAX_RELATION_REFERENCES: usize = 100;
const MAX_PROPERTY_NAME_CHARS: usize = 120;

#[derive(Clone)]
pub struct RelationConfig {
    pub target_data_source_id: String,
    dual_property: Option<RelationDualProperty>,
}

impl RelationConfig {
    /// The reciprocal property on the target source, when the relation is two-way.
    pub(crate) fn synced_property_id(&self) -> Option<&str> {
        self.dual_property
            .as_ref()
            .map(|dual| dual.synced_property_id.as_str())
    }
}

#[derive(Clone)]
struct RelationDualProperty {
    synced_property_id: String,
    synced_property_name: String,
}

#[derive(Clone)]
struct RelationProperty {
    key: String,
    id: String,
    name: String,
    config: RelationConfig,
}

#[derive(Clone, sqlx::FromRow)]
struct ActiveRelationTarget {
    id: String,
}

pub fn canonical_relation_config(value: Option<&Value>) -> Result<Value, String> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| "relation config must be an object".to_string())?;
    let data_source_id = read_string_field(object, "data_source_id", "relation.data_source_id")?;
    require_uuid(data_source_id, "relation.data_source_id")?;
    let mut canonical = Map::new();
    canonical.insert(
        "data_source_id".to_string(),
        Value::String(data_source_id.to_string()),
    );
    match object.get("dual_property") {
        None | Some(Value::Null) => {}
        Some(value) => {
            let dual = value
                .as_object()
                .ok_or_else(|| "relation.dual_property must be an object".to_string())?;
            let synced_property_id = validate_text(
                read_string_field(
                    dual,
                    "synced_property_id",
                    "relation.dual_property.synced_property_id",
                )?,
                "relation.dual_property.synced_property_id",
                80,
            )?;
            let synced_property_name = validate_text(
                read_string_field(
                    dual,
                    "synced_property_name",
                    "relation.dual_property.synced_property_name",
                )?,
                "relation.dual_property.synced_property_name",
                MAX_PROPERTY_NAME_CHARS,
            )?;
            if synced_property_id.trim().is_empty() || synced_property_name.trim().is_empty() {
                return Err("relation.dual_property fields are required".to_string());
            }
            canonical.insert(
                "dual_property".to_string(),
                json!({
                    "synced_property_id": synced_property_id,
                    "synced_property_name": synced_property_name
                }),
            );
        }
    }
    Ok(Value::Object(canonical))
}

pub fn canonical_relation_payload(value: &Value) -> Result<Value, String> {
    let values = if let Some(items) = value.as_array() {
        items
    } else if let Some(items) = value.get("relation").and_then(Value::as_array) {
        items
    } else {
        return Err("relation property must be an array of page references".to_string());
    };
    if values.len() > MAX_RELATION_REFERENCES {
        return Err("relation properties are limited to 100 pages".to_string());
    }
    let mut seen = HashSet::new();
    let mut relation = Vec::new();
    for item in values {
        let id = item
            .as_str()
            .or_else(|| item.get("id").and_then(Value::as_str))
            .ok_or_else(|| "relation page reference must include an id".to_string())?
            .trim();
        require_uuid(id, "relation page id")?;
        if seen.insert(id.to_string()) {
            relation.push(json!({ "id": id }));
        }
    }
    Ok(Value::Array(relation))
}

pub fn relation_property_value(property_id: &str, payload: Value) -> Value {
    json!({
        "id": property_id,
        "type": "relation",
        "relation": payload,
        "has_more": false
    })
}

pub fn relation_config_from_schema(schema: &Value) -> Result<Option<RelationConfig>, String> {
    if schema.get("type").and_then(Value::as_str) != Some("relation") {
        return Ok(None);
    }
    let config = schema
        .get("relation")
        .and_then(Value::as_object)
        .ok_or_else(|| "relation config must be an object".to_string())?;
    let target_data_source_id =
        read_string_field(config, "data_source_id", "relation.data_source_id")?.to_string();
    require_uuid(&target_data_source_id, "relation.data_source_id")?;
    let dual_property = config
        .get("dual_property")
        .and_then(Value::as_object)
        .map(|dual| -> Result<RelationDualProperty, String> {
            Ok(RelationDualProperty {
                synced_property_id: read_string_field(
                    dual,
                    "synced_property_id",
                    "relation.dual_property.synced_property_id",
                )?
                .to_string(),
                synced_property_name: read_string_field(
                    dual,
                    "synced_property_name",
                    "relation.dual_property.synced_property_name",
                )?
                .to_string(),
            })
        })
        .transpose()?;
    Ok(Some(RelationConfig {
        target_data_source_id,
        dual_property,
    }))
}

pub async fn ensure_relation_schema_targets_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    properties: &Value,
) -> Result<(), String> {
    let relations = relation_properties(properties)?;
    for property in &relations {
        ensure_active_data_source_tx(tx, &property.config.target_data_source_id).await?;
        if property.config.dual_property.is_some() {
            let target_schema = if property.config.target_data_source_id == source_data_source_id {
                properties.clone()
            } else {
                load_data_source_properties_tx(tx, &property.config.target_data_source_id).await?
            };
            inverse_relation_property(&property.config, &target_schema, source_data_source_id)?;
        }
    }
    Ok(())
}

/// Makes the relation values in `row_properties` the links of `source_page_id` under every
/// relation property of `schema_properties`; a relation the object omits loses its links.
///
/// Links are stored as canonical edges, so the synced side of a two-way relation follows
/// without a second write.
pub async fn replace_row_relation_links_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    source_page_id: &str,
    schema_properties: &Value,
    row_properties: &Value,
) -> Result<(), String> {
    let row_object = row_properties
        .as_object()
        .ok_or_else(|| "row page properties must be an object".to_string())?;
    for property in relation_properties(schema_properties)? {
        let value = row_object
            .get(&property.key)
            .cloned()
            .unwrap_or_else(|| relation_property_value(&property.id, Value::Array(Vec::new())));
        replace_relation_property_links_inner_tx(
            tx,
            source_data_source_id,
            source_page_id,
            &property,
            &value,
        )
        .await?;
    }
    Ok(())
}

/// Makes the relation value `row_property_value` the links of `source_page_id` under the
/// relation property `property_schema`.
pub async fn replace_relation_property_links_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    source_page_id: &str,
    property_schema: &Value,
    row_property_value: &Value,
) -> Result<(), String> {
    let property = relation_property_from_schema("", property_schema)?
        .ok_or_else(|| "row property is not a relation".to_string())?;
    replace_relation_property_links_inner_tx(
        tx,
        source_data_source_id,
        source_page_id,
        &property,
        row_property_value,
    )
    .await
}

async fn replace_relation_property_links_inner_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_data_source_id: &str,
    source_page_id: &str,
    property: &RelationProperty,
    row_property_value: &Value,
) -> Result<(), String> {
    ensure_active_data_source_tx(tx, &property.config.target_data_source_id).await?;
    let current_target_ids = relation_ids_from_value(row_property_value)?;
    let targets = active_relation_targets_tx(
        tx,
        &property.config.target_data_source_id,
        &current_target_ids,
    )
    .await?;
    let target_ids = targets
        .into_iter()
        .map(|target| target.id)
        .collect::<Vec<_>>();
    let (added, removed) = page_property_storage::replace_relation_targets(
        tx,
        source_data_source_id,
        source_page_id,
        &property.id,
        &target_ids,
    )
    .await?;
    if added.is_empty() && removed.is_empty() {
        return Ok(());
    }
    data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(tx, source_data_source_id)
        .await?;
    data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(
        tx,
        &property.config.target_data_source_id,
    )
    .await?;
    Ok(())
}

pub async fn hydrate_relation_titles_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &mut [NotePageRow],
) -> Result<(), String> {
    let mut target_ids = HashSet::new();
    for row in rows.iter() {
        collect_relation_target_ids(
            &parse_json(&row.properties, "row page properties")?,
            &mut target_ids,
        );
    }
    if target_ids.is_empty() {
        return Ok(());
    }
    let mut titles = HashMap::new();
    for target_id in target_ids {
        if let Some(title) = sqlx::query_scalar::<_, String>(
            "SELECT title FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(&target_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes relation target title: {e}"))?
        {
            titles.insert(target_id, title);
        }
    }
    for row in rows {
        let mut properties = parse_json(&row.properties, "row page properties")?;
        hydrate_relation_titles_in_value(&mut properties, &titles);
        row.properties = properties.to_string();
    }
    Ok(())
}

fn relation_properties(properties: &Value) -> Result<Vec<RelationProperty>, String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut relations = Vec::new();
    for (key, value) in object {
        if let Some(property) = relation_property_from_schema(key, value)? {
            relations.push(property);
        }
    }
    Ok(relations)
}

fn relation_property_from_schema(
    key: &str,
    value: &Value,
) -> Result<Option<RelationProperty>, String> {
    let Some(config) = relation_config_from_schema(value)? else {
        return Ok(None);
    };
    let object = value
        .as_object()
        .ok_or_else(|| "data source property must be an object".to_string())?;
    Ok(Some(RelationProperty {
        key: if key.is_empty() {
            read_string_field(object, "name", "property.name")?.to_string()
        } else {
            key.to_string()
        },
        id: read_string_field(object, "id", "property.id")?.to_string(),
        name: read_string_field(object, "name", "property.name")?.to_string(),
        config,
    }))
}

fn inverse_relation_property(
    relation: &RelationConfig,
    target_schema: &Value,
    source_data_source_id: &str,
) -> Result<RelationProperty, String> {
    let Some(dual) = relation.dual_property.as_ref() else {
        return Err("relation dual_property is not configured".to_string());
    };
    let properties = relation_properties(target_schema)?;
    let property = properties
        .into_iter()
        .find(|property| property.id == dual.synced_property_id)
        .ok_or_else(|| "relation dual_property target was not found".to_string())?;
    if property.config.target_data_source_id != source_data_source_id {
        return Err("relation dual_property target must point back to the source".to_string());
    }
    if property.name != dual.synced_property_name {
        return Err("relation dual_property name must match the target property".to_string());
    }
    Ok(property)
}

fn relation_ids_from_value(value: &Value) -> Result<Vec<String>, String> {
    let canonical = canonical_relation_payload(value.get("relation").unwrap_or(value))?;
    Ok(canonical
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .collect())
}

async fn ensure_active_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check notes relation data source: {e}"))?;
    if exists.is_none() {
        return Err("relation target data source not found".to_string());
    }
    Ok(())
}

async fn active_relation_targets_tx(
    tx: &mut Transaction<'_, Sqlite>,
    target_data_source_id: &str,
    target_page_ids: &[String],
) -> Result<Vec<ActiveRelationTarget>, String> {
    let mut targets = Vec::with_capacity(target_page_ids.len());
    for target_page_id in target_page_ids {
        let target = sqlx::query_as::<_, ActiveRelationTarget>(
            "SELECT page.id
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
        .map_err(|e| format!("load notes relation target: {e}"))?
        .ok_or_else(|| {
            "relation target page must belong to the configured data source".to_string()
        })?;
        targets.push(target);
    }
    Ok(targets)
}

async fn load_data_source_properties_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<Value, String> {
    let properties: String = sqlx::query_scalar(concat!(
        "SELECT ",
        crate::note_data_source_properties_json!("data_source"),
        " FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0"
    ))
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes relation target data source: {e}"))?
    .ok_or_else(|| "relation target data source not found".to_string())?;
    parse_json(&properties, "relation target data source properties")
}

fn collect_relation_target_ids(value: &Value, target_ids: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("relation") {
                if let Some(items) = object.get("relation").and_then(Value::as_array) {
                    for item in items {
                        if let Some(id) = item.get("id").and_then(Value::as_str) {
                            target_ids.insert(id.to_string());
                        }
                    }
                }
            }
            for nested in object.values() {
                collect_relation_target_ids(nested, target_ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_relation_target_ids(item, target_ids);
            }
        }
        _ => {}
    }
}

fn hydrate_relation_titles_in_value(value: &mut Value, titles: &HashMap<String, String>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("relation") {
                if let Some(items) = object.get_mut("relation").and_then(Value::as_array_mut) {
                    for item in items {
                        if let Some(item_object) = item.as_object_mut() {
                            if let Some(id) = item_object.get("id").and_then(Value::as_str) {
                                if let Some(title) = titles.get(id) {
                                    item_object
                                        .insert("title".to_string(), Value::String(title.clone()));
                                }
                            }
                        }
                    }
                }
            }
            for nested in object.values_mut() {
                hydrate_relation_titles_in_value(nested, titles);
            }
        }
        Value::Array(items) => {
            for item in items {
                hydrate_relation_titles_in_value(item, titles);
            }
        }
        _ => {}
    }
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

fn validate_text(value: &str, label: &str, max_chars: usize) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label} is too long"));
    }
    Ok(value.trim().to_string())
}

fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}
