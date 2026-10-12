use serde_json::Value;
use sqlx::{Row, SqlitePool};

use std::collections::BTreeSet;

use super::CalendarIcalendarExportMetadata;

pub(super) fn calendar_icalendar_export_metadata(
    methods: Vec<String>,
) -> CalendarIcalendarExportMetadata {
    let distinct_methods = methods
        .into_iter()
        .map(|method| method.trim().to_ascii_uppercase())
        .filter(|method| !method.is_empty())
        .collect::<BTreeSet<_>>();
    CalendarIcalendarExportMetadata {
        method: if distinct_methods.len() == 1 {
            distinct_methods.iter().next().cloned()
        } else {
            None
        },
        mixed_methods: distinct_methods.len() > 1,
    }
}

pub(super) async fn load_component_jcal(
    pool: &SqlitePool,
    component_id: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(component_id) = component_id else {
        return Ok(None);
    };
    let value = load_component_value(pool, component_id).await?;
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|e| format!("serialize iCalendar component: {e}"))
}

pub(super) fn load_component_value<'a>(
    pool: &'a SqlitePool,
    component_id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + 'a>> {
    Box::pin(async move {
        let row = sqlx::query("SELECT component_type FROM icalendar_components WHERE id = ?")
            .bind(component_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load iCalendar component: {e}"))?
            .ok_or_else(|| format!("iCalendar component not found: {component_id}"))?;
        let component_type: String = row
            .try_get("component_type")
            .map_err(|e| format!("read component_type: {e}"))?;

        let property_rows = sqlx::query(
            "SELECT id, name, value_type
         FROM icalendar_component_properties
         WHERE component_id = ?
         ORDER BY sort_order ASC",
        )
        .bind(component_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load iCalendar properties: {e}"))?;

        let mut properties = Vec::with_capacity(property_rows.len());
        for property_row in property_rows {
            let property_id: String = property_row
                .try_get("id")
                .map_err(|e| format!("read property id: {e}"))?;
            let name: String = property_row
                .try_get("name")
                .map_err(|e| format!("read property name: {e}"))?;
            let value_type: String = property_row
                .try_get("value_type")
                .map_err(|e| format!("read property value_type: {e}"))?;
            let params = load_property_params(pool, &property_id).await?;
            let mut property = vec![Value::String(name), params, Value::String(value_type)];
            property.extend(load_property_values(pool, &property_id).await?);
            properties.push(Value::Array(property));
        }

        let child_ids = sqlx::query_scalar::<_, String>(
            "SELECT id FROM icalendar_components
         WHERE parent_component_id = ?
         ORDER BY sort_order ASC",
        )
        .bind(component_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load child iCalendar components: {e}"))?;
        let mut children = Vec::with_capacity(child_ids.len());
        for child_id in child_ids {
            children.push(load_component_value(pool, &child_id).await?);
        }

        Ok(Value::Array(vec![
            Value::String(component_type),
            Value::Array(properties),
            Value::Array(children),
        ]))
    })
}

async fn load_property_params(pool: &SqlitePool, property_id: &str) -> Result<Value, String> {
    let rows = sqlx::query(
        "SELECT id, name FROM icalendar_property_parameters
         WHERE property_id = ?
         ORDER BY sort_order ASC",
    )
    .bind(property_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load iCalendar parameters: {e}"))?;
    let mut params = serde_json::Map::new();
    for row in rows {
        let parameter_id: String = row
            .try_get("id")
            .map_err(|e| format!("read parameter id: {e}"))?;
        let name: String = row
            .try_get("name")
            .map_err(|e| format!("read parameter name: {e}"))?;
        let values = load_parameter_values(pool, &parameter_id).await?;
        let value = if values.len() == 1 {
            values.into_iter().next().unwrap_or(Value::Null)
        } else {
            Value::Array(values)
        };
        params.insert(name, value);
    }
    Ok(Value::Object(params))
}

async fn load_property_values(pool: &SqlitePool, property_id: &str) -> Result<Vec<Value>, String> {
    load_root_values(pool, Some(property_id), None).await
}

async fn load_parameter_values(
    pool: &SqlitePool,
    parameter_id: &str,
) -> Result<Vec<Value>, String> {
    load_root_values(pool, None, Some(parameter_id)).await
}

async fn load_root_values(
    pool: &SqlitePool,
    property_id: Option<&str>,
    parameter_id: Option<&str>,
) -> Result<Vec<Value>, String> {
    let (query, bind_value) = if let Some(property_id) = property_id {
        (
            "SELECT id FROM icalendar_value_nodes
             WHERE property_id = ? AND parent_node_id IS NULL
             ORDER BY sort_order ASC",
            property_id,
        )
    } else if let Some(parameter_id) = parameter_id {
        (
            "SELECT id FROM icalendar_value_nodes
             WHERE parameter_id = ? AND parent_node_id IS NULL
             ORDER BY sort_order ASC",
            parameter_id,
        )
    } else {
        return Ok(Vec::new());
    };
    let ids = sqlx::query_scalar::<_, String>(query)
        .bind(bind_value)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load iCalendar value roots: {e}"))?;
    let mut values = Vec::with_capacity(ids.len());
    for id in ids {
        values.push(load_value_node(pool, &id).await?);
    }
    Ok(values)
}

fn load_value_node<'a>(
    pool: &'a SqlitePool,
    node_id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + 'a>> {
    Box::pin(async move {
        let row = sqlx::query(
            "SELECT value_kind, text_value, number_value, boolean_value
         FROM icalendar_value_nodes
         WHERE id = ?",
        )
        .bind(node_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("load iCalendar value node: {e}"))?;
        let value_kind: String = row
            .try_get("value_kind")
            .map_err(|e| format!("read value_kind: {e}"))?;
        match value_kind.as_str() {
            "array" => {
                let child_ids = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM icalendar_value_nodes
                 WHERE parent_node_id = ?
                 ORDER BY sort_order ASC",
                )
                .bind(node_id)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("load iCalendar value children: {e}"))?;
                let mut children = Vec::with_capacity(child_ids.len());
                for child_id in child_ids {
                    children.push(load_value_node(pool, &child_id).await?);
                }
                Ok(Value::Array(children))
            }
            "object" => {
                let child_rows = sqlx::query(
                    "SELECT id, object_key FROM icalendar_value_nodes
                 WHERE parent_node_id = ?
                 ORDER BY sort_order ASC",
                )
                .bind(node_id)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("load iCalendar object values: {e}"))?;
                let mut object = serde_json::Map::new();
                for child_row in child_rows {
                    let child_id: String = child_row
                        .try_get("id")
                        .map_err(|e| format!("read object child id: {e}"))?;
                    let object_key: Option<String> = child_row
                        .try_get("object_key")
                        .map_err(|e| format!("read object_key: {e}"))?;
                    if let Some(key) = object_key {
                        object.insert(key, load_value_node(pool, &child_id).await?);
                    }
                }
                Ok(Value::Object(object))
            }
            "text" => {
                let value: Option<String> = row
                    .try_get("text_value")
                    .map_err(|e| format!("read text_value: {e}"))?;
                Ok(Value::String(value.unwrap_or_default()))
            }
            "number" => {
                let value: Option<f64> = row
                    .try_get("number_value")
                    .map_err(|e| format!("read number_value: {e}"))?;
                Ok(serde_json::Number::from_f64(value.unwrap_or(0.0))
                    .map(Value::Number)
                    .unwrap_or(Value::Null))
            }
            "boolean" => {
                let value: Option<i64> = row
                    .try_get("boolean_value")
                    .map_err(|e| format!("read boolean_value: {e}"))?;
                Ok(Value::Bool(value.unwrap_or(0) != 0))
            }
            "null" => Ok(Value::Null),
            _ => Err(format!("unsupported iCalendar value kind: {value_kind}")),
        }
    })
}
