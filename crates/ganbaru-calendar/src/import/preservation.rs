use std::collections::HashMap;

use serde_json::Value;
use sqlx::{Sqlite, Transaction};

use super::{CalendarImportPreservation, CalendarImportPreservedComponent};

/// Calendar row a preserved component projects into, written with the component itself.
pub(super) struct ProjectedRow {
    pub(super) kind: &'static str,
    pub(super) id: String,
}

/// What the import decided about the preserved components before they are stored.
#[derive(Default)]
pub(super) struct PreservationOutcome {
    /// Projected row by component id.
    pub(super) projections: HashMap<String, ProjectedRow>,
    /// Extra diagnostics by preserved object id, appended after the source diagnostics.
    pub(super) diagnostics: HashMap<String, Vec<String>>,
}

/// Replaces the preserved objects of one source, writing each component's projection and each
/// object's extra diagnostics in the same inserts.
pub(super) async fn replace_preservation(
    tx: &mut Transaction<'_, Sqlite>,
    target_calendar_id: &str,
    now: &str,
    preservation: &CalendarImportPreservation,
    outcome: &PreservationOutcome,
) -> Result<(), String> {
    let projections = &outcome.projections;
    sqlx::query(
        "DELETE FROM icalendar_objects
         WHERE calendar_id = ? AND source_kind = ? AND source_name = ?",
    )
    .bind(target_calendar_id)
    .bind(&preservation.source_kind)
    .bind(&preservation.source_name)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("delete existing iCalendar preservation: {e}"))?;

    for object in &preservation.objects {
        sqlx::query(
            "INSERT INTO icalendar_objects
                (id, calendar_id, source_kind, source_name, source_fingerprint,
                 prodid, version, method, calendar_scale, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&object.id)
        .bind(target_calendar_id)
        .bind(&preservation.source_kind)
        .bind(&preservation.source_name)
        .bind(&preservation.source_fingerprint)
        .bind(&object.prodid)
        .bind(&object.version)
        .bind(&object.method)
        .bind(&object.calendar_scale)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar object preservation: {e}"))?;

        let mut diagnostics =
            parse_string_vec(&object.diagnostics, "preservation.object.diagnostics")?;
        if let Some(extra) = outcome.diagnostics.get(&object.id) {
            diagnostics.extend(extra.iter().cloned());
        }
        for (sort_order, message) in diagnostics.iter().enumerate() {
            sqlx::query(
                "INSERT INTO icalendar_object_diagnostics
                    (id, object_id, message, sort_order)
                 VALUES (lower(hex(randomblob(16))), ?, ?, ?)",
            )
            .bind(&object.id)
            .bind(message)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert iCalendar object diagnostic: {e}"))?;
        }

        let root_component_id = format!("{}:root", object.id);
        insert_preserved_component_row(
            tx,
            target_calendar_id,
            now,
            &object.id,
            None,
            0,
            &root_component_id,
            "vcalendar",
            None,
            None,
            None,
            None,
            None,
            "lossless",
            None,
            &[],
            &object.raw_jcal,
        )
        .await?;
        insert_preserved_components(
            tx,
            target_calendar_id,
            now,
            &object.id,
            Some(root_component_id),
            &object.components,
            projections,
        )
        .await?;
    }
    Ok(())
}

async fn insert_preserved_components(
    tx: &mut Transaction<'_, Sqlite>,
    target_calendar_id: &str,
    now: &str,
    object_id: &str,
    root_component_id: Option<String>,
    components: &[CalendarImportPreservedComponent],
    projections: &HashMap<String, ProjectedRow>,
) -> Result<(), String> {
    let mut stack: Vec<(Option<String>, i64, &CalendarImportPreservedComponent)> = components
        .iter()
        .enumerate()
        .rev()
        .map(|(index, component)| (root_component_id.clone(), index as i64, component))
        .collect();

    while let Some((parent_component_id, sort_order, component)) = stack.pop() {
        let warnings = parse_string_vec(
            &component.projection_warnings,
            "preservation.component.projection_warnings",
        )?;
        insert_preserved_component_row(
            tx,
            target_calendar_id,
            now,
            object_id,
            parent_component_id.as_deref(),
            sort_order,
            &component.id,
            &component.component_type,
            component.uid.as_deref(),
            component.recurrence_id.as_deref(),
            component.recurrence_id_value_type.as_deref(),
            component.sequence,
            component.dtstart_key.as_deref(),
            &component.preservation_status,
            projections.get(&component.id),
            &warnings,
            &component.raw_jcal,
        )
        .await?;

        for (index, child) in component.components.iter().enumerate().rev() {
            stack.push((Some(component.id.clone()), index as i64, child));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_preserved_component_row(
    tx: &mut Transaction<'_, Sqlite>,
    target_calendar_id: &str,
    now: &str,
    object_id: &str,
    parent_component_id: Option<&str>,
    sort_order: i64,
    component_id: &str,
    component_type: &str,
    uid: Option<&str>,
    recurrence_id: Option<&str>,
    recurrence_id_value_type: Option<&str>,
    sequence: Option<i64>,
    dtstart_key: Option<&str>,
    preservation_status: &str,
    projection: Option<&ProjectedRow>,
    projection_warnings: &[String],
    raw_jcal: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO icalendar_components
            (id, object_id, parent_component_id, calendar_id, component_type,
             uid, recurrence_id, recurrence_id_value_type, sequence, dtstart_key,
             projected_kind, projected_id, preservation_status, sort_order,
             created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(component_id)
    .bind(object_id)
    .bind(parent_component_id)
    .bind(target_calendar_id)
    .bind(component_type)
    .bind(uid)
    .bind(recurrence_id)
    .bind(recurrence_id_value_type)
    .bind(sequence)
    .bind(dtstart_key)
    .bind(projection.map(|row| row.kind))
    .bind(projection.map(|row| row.id.as_str()))
    .bind(preservation_status)
    .bind(sort_order)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert iCalendar component preservation: {e}"))?;

    for (index, message) in projection_warnings.iter().enumerate() {
        sqlx::query(
            "INSERT INTO icalendar_component_projection_warnings
                (id, component_id, message, sort_order)
             VALUES (lower(hex(randomblob(16))), ?, ?, ?)",
        )
        .bind(component_id)
        .bind(message)
        .bind(index as i64)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar projection warning: {e}"))?;
    }

    store_jcal_component(tx, component_id, raw_jcal).await
}

async fn store_jcal_component(
    tx: &mut Transaction<'_, Sqlite>,
    component_id: &str,
    raw_jcal: &str,
) -> Result<(), String> {
    let value = serde_json::from_str::<Value>(raw_jcal)
        .map_err(|e| format!("parse preserved iCalendar component: {e}"))?;
    let component = value
        .as_array()
        .ok_or_else(|| "preserved iCalendar component must be an array".to_string())?;
    let properties = component
        .get(1)
        .and_then(Value::as_array)
        .ok_or_else(|| "preserved iCalendar component properties must be an array".to_string())?;

    for (property_index, property) in properties.iter().enumerate() {
        let Some(property_array) = property.as_array() else {
            continue;
        };
        if property_array.len() < 3 {
            continue;
        }
        let Some(name) = property_array.first().and_then(Value::as_str) else {
            continue;
        };
        let Some(value_type) = property_array.get(2).and_then(Value::as_str) else {
            continue;
        };
        let property_id = format!("{component_id}:property:{property_index}");
        sqlx::query(
            "INSERT INTO icalendar_component_properties
                (id, component_id, name, value_type, sort_order)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&property_id)
        .bind(component_id)
        .bind(name)
        .bind(value_type)
        .bind(property_index as i64)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar property: {e}"))?;

        if let Some(params) = property_array.get(1).and_then(Value::as_object) {
            for (param_index, (param_name, param_value)) in params.iter().enumerate() {
                let parameter_id = format!("{property_id}:parameter:{param_index}");
                sqlx::query(
                    "INSERT INTO icalendar_property_parameters
                        (id, property_id, name, sort_order)
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&parameter_id)
                .bind(&property_id)
                .bind(param_name)
                .bind(param_index as i64)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("insert iCalendar parameter: {e}"))?;
                store_value_node(
                    tx,
                    param_value,
                    ValueNodeInsert {
                        property_id: Some(&property_id),
                        parameter_id: Some(&parameter_id),
                        parent_node_id: None,
                        sort_order: 0,
                        node_id: format!("{parameter_id}:value"),
                        object_key: None,
                    },
                )
                .await?;
            }
        }

        for (value_index, property_value) in property_array.iter().skip(3).enumerate() {
            store_value_node(
                tx,
                property_value,
                ValueNodeInsert {
                    property_id: Some(&property_id),
                    parameter_id: None,
                    parent_node_id: None,
                    sort_order: value_index as i64,
                    node_id: format!("{property_id}:value:{value_index}"),
                    object_key: None,
                },
            )
            .await?;
        }
    }
    Ok(())
}

struct ValueNodeInsert<'a> {
    property_id: Option<&'a str>,
    parameter_id: Option<&'a str>,
    parent_node_id: Option<String>,
    sort_order: i64,
    node_id: String,
    object_key: Option<String>,
}

fn store_value_node<'a>(
    tx: &'a mut Transaction<'_, Sqlite>,
    value: &'a Value,
    target: ValueNodeInsert<'a>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let (value_kind, text_value, number_value, boolean_value) = match value {
            Value::Array(_) => ("array", None, None, None),
            Value::Object(_) => ("object", None, None, None),
            Value::String(text) => ("text", Some(text.as_str()), None, None),
            Value::Number(number) => ("number", None, number.as_f64(), None),
            Value::Bool(boolean) => (
                "boolean",
                None,
                None,
                Some(if *boolean { 1_i64 } else { 0_i64 }),
            ),
            Value::Null => ("null", None, None, None),
        };
        sqlx::query(
            "INSERT INTO icalendar_value_nodes
                (id, property_id, parameter_id, parent_node_id, sort_order,
                 value_kind, object_key, text_value, number_value, boolean_value)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&target.node_id)
        .bind(target.property_id)
        .bind(target.parameter_id)
        .bind(target.parent_node_id.as_deref())
        .bind(target.sort_order)
        .bind(value_kind)
        .bind(target.object_key.as_deref())
        .bind(text_value)
        .bind(number_value)
        .bind(boolean_value)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar value: {e}"))?;

        if let Value::Array(items) = value {
            for (index, child) in items.iter().enumerate() {
                store_value_node(
                    tx,
                    child,
                    ValueNodeInsert {
                        property_id: target.property_id,
                        parameter_id: target.parameter_id,
                        parent_node_id: Some(target.node_id.clone()),
                        sort_order: index as i64,
                        node_id: format!("{}:child:{index}", target.node_id),
                        object_key: None,
                    },
                )
                .await?;
            }
        }
        if let Value::Object(map) = value {
            for (index, (key, child)) in map.iter().enumerate() {
                store_value_node(
                    tx,
                    child,
                    ValueNodeInsert {
                        property_id: target.property_id,
                        parameter_id: target.parameter_id,
                        parent_node_id: Some(target.node_id.clone()),
                        sort_order: index as i64,
                        node_id: format!("{}:member:{index}", target.node_id),
                        object_key: Some(key.clone()),
                    },
                )
                .await?;
            }
        }
        Ok(())
    })
}

pub(super) fn preservation_link_id(value: &Option<String>, apply: bool) -> Option<&str> {
    if !apply {
        return None;
    }
    value.as_deref().filter(|id| !id.trim().is_empty())
}

fn parse_string_vec(value: &str, field: &str) -> Result<Vec<String>, String> {
    serde_json::from_str::<Vec<String>>(value)
        .map_err(|e| format!("{field} is not a valid string list: {e}"))
}
