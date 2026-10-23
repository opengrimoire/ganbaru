use std::collections::HashMap;

mod graph;
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
    // One random id per import command groups the objects it wrote, so export can tell one
    // multi-calendar import apart from the same source imported again on another replica.
    let import_id: String = sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("generate iCalendar import id: {e}"))?;
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
                 prodid, version, method, calendar_scale, created_at, updated_at, import_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(&import_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar object preservation: {e}"))?;

        let mut diagnostics =
            parse_string_vec(&object.diagnostics, "preservation.object.diagnostics")?;
        if let Some(extra) = outcome.diagnostics.get(&object.id) {
            diagnostics.extend(extra.iter().cloned());
        }
        let diagnostics = graph::bound_diagnostics(&object.id, diagnostics);
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

    let properties = graph::plan_graph(component_id, raw_jcal)?;
    let bounded = graph::bound_graph(component_id, projection_warnings.to_vec(), properties);
    for (index, message) in bounded.warnings.iter().enumerate() {
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
    for planned in &bounded.properties {
        store_property(tx, component_id, planned).await?;
    }
    Ok(())
}

/// Writes one planned property with its parameters and value nodes.
async fn store_property(
    tx: &mut Transaction<'_, Sqlite>,
    component_id: &str,
    planned: &graph::PlannedProperty,
) -> Result<(), String> {
    let property = &planned.property;
    sqlx::query(
        "INSERT INTO icalendar_component_properties
            (id, component_id, name, value_type, sort_order)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&property.id)
    .bind(component_id)
    .bind(&property.name)
    .bind(&property.value_type)
    .bind(property.sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert iCalendar property: {e}"))?;
    for parameter in &planned.parameters {
        sqlx::query(
            "INSERT INTO icalendar_property_parameters
                (component_id, id, property_id, name, sort_order)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(component_id)
        .bind(&parameter.id)
        .bind(&parameter.property_id)
        .bind(&parameter.name)
        .bind(parameter.sort_order)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar parameter: {e}"))?;
    }
    for node in &planned.values {
        sqlx::query(
            "INSERT INTO icalendar_value_nodes
                (component_id, id, property_id, parameter_id, parent_node_id, sort_order,
                 value_kind, object_key, text_value, number_value, boolean_value)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(component_id)
        .bind(&node.id)
        .bind(&node.property_id)
        .bind(&node.parameter_id)
        .bind(&node.parent_node_id)
        .bind(node.sort_order)
        .bind(node.value_kind)
        .bind(&node.object_key)
        .bind(&node.text_value)
        .bind(&node.number_value)
        .bind(node.boolean_value)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert iCalendar value: {e}"))?;
    }
    Ok(())
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
