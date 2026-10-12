use crate::data_sources;
use crate::models::{NoteDataSourceButtonClick, NoteDataSourceRowPropertyUpdate, NotePageDto};
use crate::validation::require_uuid;
use serde_json::{Map, Value, json};
use sqlx::SqlitePool;
use std::collections::HashMap;

const MAX_BUTTON_LABEL_CHARS: usize = 120;
const MAX_BUTTON_ACTIONS: usize = 5;
const BUTTON_EDITABLE_PROPERTY_TYPES: &[&str] = &[
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
    "place",
    "relation",
];
const BROAD_OR_DESTRUCTIVE_ACTION_TYPES: &[&str] = &[
    "add_page_to_data_source",
    "delete_pages",
    "edit_pages_in_data_source",
    "trash_pages",
    "update_matching_rows",
];

#[derive(Clone)]
struct ButtonAction {
    property_id: String,
    value: Value,
}

struct ButtonConfig {
    label: String,
    requires_confirmation: bool,
    actions: Vec<ButtonAction>,
}

pub fn canonical_button_config(value: Option<&Value>) -> Result<Value, String> {
    let object = value.and_then(Value::as_object);
    let label = normalize_label(
        object
            .and_then(|config| config.get("label"))
            .and_then(Value::as_str)
            .unwrap_or("Run"),
    )?;
    let requires_confirmation = object
        .and_then(|config| config.get("requires_confirmation"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let actions = object
        .and_then(|config| config.get("actions"))
        .and_then(Value::as_array)
        .map(|items| canonical_button_actions(items, requires_confirmation))
        .transpose()?
        .unwrap_or_default();
    Ok(json!({
        "label": label,
        "requires_confirmation": requires_confirmation,
        "actions": actions
    }))
}

pub fn ensure_button_schema(properties: &Value) -> Result<(), String> {
    let properties_object = properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut by_id = HashMap::new();
    for property in properties_object.values() {
        let object = property
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        let id = read_string_field(object, "id", "property.id")?;
        let name = read_string_field(object, "name", "property.name")?;
        let property_type = read_string_field(object, "type", "property.type")?;
        by_id.insert(
            id.to_string(),
            (name.to_string(), property_type.to_string()),
        );
    }
    for property in properties_object.values() {
        let object = property
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        if object.get("type").and_then(Value::as_str) != Some("button") {
            continue;
        }
        let button_id = read_string_field(object, "id", "property.id")?;
        let config = button_config_from_property(object)?;
        for action in &config.actions {
            if action.property_id == button_id {
                return Err("button action cannot update its own button property".to_string());
            }
            let Some((_name, property_type)) = by_id.get(&action.property_id) else {
                return Err("button action references an unknown property".to_string());
            };
            if !BUTTON_EDITABLE_PROPERTY_TYPES.contains(&property_type.as_str()) {
                return Err("button action target property is read-only".to_string());
            }
        }
    }
    Ok(())
}

pub fn hydrate_buttons(
    schema_properties: &Value,
    rows: &mut [crate::models::NotePageRow],
) -> Result<(), String> {
    let buttons = button_properties_from_schema(schema_properties)?;
    if buttons.is_empty() {
        return Ok(());
    }
    for row in rows {
        let mut properties = serde_json::from_str::<Value>(&row.properties)
            .map_err(|e| format!("parse row page properties: {e}"))?;
        let object = properties
            .as_object_mut()
            .ok_or_else(|| "row page properties must be an object".to_string())?;
        for button in &buttons {
            object.insert(
                button.key.clone(),
                button_property_value(&button.id, &button.label),
            );
        }
        row.properties = properties.to_string();
    }
    Ok(())
}

pub fn button_property_value(property_id: &str, label: &str) -> Value {
    json!({
        "id": property_id,
        "type": "button",
        "button": {
            "label": label
        }
    })
}

pub fn button_plain_text(payload: &Value) -> String {
    payload
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub async fn click_data_source_button(
    pool: &SqlitePool,
    data_source_id: &str,
    page_id: &str,
    request: NoteDataSourceButtonClick,
) -> Result<NotePageDto, String> {
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    require_uuid(data_source_id, "data_source_id")?;
    require_uuid(page_id, "page_id")?;
    let property_id = request.property_id.trim();
    if property_id.is_empty() {
        return Err("button property_id is required".to_string());
    }
    let schema_properties: String = sqlx::query_scalar(
        "SELECT data_source.properties
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         JOIN notes_pages AS page ON page.parent_data_source_id = data_source.id
         WHERE data_source.id = ?
           AND page.id = ?
           AND page.parent_type = 'data_source_id'
           AND data_source.in_trash = 0
           AND database.in_trash = 0
           AND page.in_trash = 0
           AND page.archived = 0",
    )
    .bind(data_source_id.trim())
    .bind(page_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes data source button schema: {e}"))?
    .ok_or_else(|| "row page not found".to_string())?;
    let schema: Value = serde_json::from_str(&schema_properties)
        .map_err(|e| format!("parse data source properties: {e}"))?;
    let button_property = find_button_property(&schema, property_id)?;
    let config = button_config_from_property(button_property)?;
    if config.requires_confirmation && request.confirmed != Some(true) {
        return Err("button action requires confirmation".to_string());
    }
    if config.actions.is_empty() {
        return Err("button has no configured action".to_string());
    }
    let mut updated = None;
    for action in config.actions {
        updated = Some(
            data_sources::layouts::table::update_data_source_row_property(
                pool,
                data_source_id,
                page_id,
                NoteDataSourceRowPropertyUpdate {
                    property_id: action.property_id,
                    value: action.value,
                },
            )
            .await?,
        );
    }
    updated.ok_or_else(|| "button did not run an action".to_string())
}

fn canonical_button_actions(
    actions: &[Value],
    requires_confirmation: bool,
) -> Result<Vec<Value>, String> {
    if actions.len() > MAX_BUTTON_ACTIONS {
        return Err("button actions are limited to 5".to_string());
    }
    let mut canonical = Vec::with_capacity(actions.len());
    for action in actions {
        canonical.push(canonical_button_action(action, requires_confirmation)?);
    }
    Ok(canonical)
}

fn canonical_button_action(value: &Value, requires_confirmation: bool) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "button action must be an object".to_string())?;
    let action_type = read_string_field(object, "type", "button action.type")?;
    if action_type != "update_current_row_property" {
        if BROAD_OR_DESTRUCTIVE_ACTION_TYPES.contains(&action_type) && !requires_confirmation {
            return Err("broad or destructive button actions require confirmation".to_string());
        }
        return Err("button action type is not supported".to_string());
    }
    let property_id = validate_text(
        read_string_field(object, "property_id", "button action.property_id")?.trim(),
        "button action.property_id",
        80,
    )?;
    if property_id.is_empty() {
        return Err("button action.property_id is required".to_string());
    }
    let property_name = validate_text(
        object
            .get("property_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim(),
        "button action.property_name",
        120,
    )?;
    let property_type = read_string_field(object, "property_type", "button action.property_type")?;
    if !BUTTON_EDITABLE_PROPERTY_TYPES.contains(&property_type) {
        return Err("button action.property_type is not editable".to_string());
    }
    let action_value = object
        .get("value")
        .cloned()
        .ok_or_else(|| "button action.value is required".to_string())?;
    validate_action_value(property_type, &action_value)?;
    Ok(json!({
        "type": "update_current_row_property",
        "property_id": property_id,
        "property_name": property_name,
        "property_type": property_type,
        "value": action_value
    }))
}

fn validate_action_value(property_type: &str, value: &Value) -> Result<(), String> {
    match property_type {
        "title" | "rich_text" => {
            let text = value
                .as_str()
                .ok_or_else(|| "button text action value must be a string".to_string())?;
            validate_text(text, "button action.value", 2_000).map(|_| ())
        }
        "number" => {
            if value.is_null() || value.is_number() {
                return Ok(());
            }
            let text = value
                .as_str()
                .ok_or_else(|| "button number action value must be a number or text".to_string())?;
            if text.trim().is_empty() || text.trim().parse::<f64>().is_ok() {
                Ok(())
            } else {
                Err("button number action value must be numeric".to_string())
            }
        }
        "checkbox" => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| "button checkbox action value must be boolean".to_string()),
        "select" | "status" => {
            if value.is_null() || value.is_string() || value.is_object() {
                Ok(())
            } else {
                Err("button option action value must be text, object, or null".to_string())
            }
        }
        "multi_select" | "relation" => {
            if value.is_string() || value.is_array() {
                Ok(())
            } else {
                Err("button list action value must be text or an array".to_string())
            }
        }
        "date" | "place" => {
            if value.is_null() || value.is_string() || value.is_object() {
                Ok(())
            } else {
                Err("button date or place action value must be text, object, or null".to_string())
            }
        }
        "url" | "email" | "phone_number" => {
            if value.is_null() {
                return Ok(());
            }
            let text = value
                .as_str()
                .ok_or_else(|| "button URL action value must be text or null".to_string())?;
            validate_text(text, "button action.value", 2_000).map(|_| ())
        }
        other => Err(format!("button action cannot edit {other} properties")),
    }
}

fn find_button_property<'a>(
    schema: &'a Value,
    property_id: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    for value in object.values() {
        let property = value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        if property.get("id").and_then(Value::as_str) == Some(property_id)
            && property.get("type").and_then(Value::as_str) == Some("button")
        {
            return Ok(property);
        }
    }
    Err("button property not found".to_string())
}

fn button_properties_from_schema(schema: &Value) -> Result<Vec<ButtonProperty>, String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut buttons = Vec::new();
    for (key, value) in object {
        let property = value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        if property.get("type").and_then(Value::as_str) != Some("button") {
            continue;
        }
        let config = button_config_from_property(property)?;
        buttons.push(ButtonProperty {
            key: key.clone(),
            id: read_string_field(property, "id", "property.id")?.to_string(),
            label: config.label,
        });
    }
    Ok(buttons)
}

struct ButtonProperty {
    key: String,
    id: String,
    label: String,
}

fn button_config_from_property(property: &Map<String, Value>) -> Result<ButtonConfig, String> {
    let config = property
        .get("button")
        .and_then(Value::as_object)
        .ok_or_else(|| "button config must be an object".to_string())?;
    let label = normalize_label(config.get("label").and_then(Value::as_str).unwrap_or("Run"))?;
    let requires_confirmation = config
        .get("requires_confirmation")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let actions = config
        .get("actions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| parse_button_action(item, requires_confirmation))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(ButtonConfig {
        label,
        requires_confirmation,
        actions,
    })
}

fn parse_button_action(value: &Value, requires_confirmation: bool) -> Result<ButtonAction, String> {
    let canonical = canonical_button_action(value, requires_confirmation)?;
    let object = canonical
        .as_object()
        .ok_or_else(|| "button action must be an object".to_string())?;
    Ok(ButtonAction {
        property_id: read_string_field(object, "property_id", "button action.property_id")?
            .to_string(),
        value: object
            .get("value")
            .cloned()
            .ok_or_else(|| "button action.value is required".to_string())?,
    })
}

fn normalize_label(value: &str) -> Result<String, String> {
    let label = validate_text(value.trim(), "button.label", MAX_BUTTON_LABEL_CHARS)?;
    if label.is_empty() {
        Ok("Run".to_string())
    } else {
        Ok(label)
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
    Ok(value.to_string())
}
